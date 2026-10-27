# Ejecución de futuros: garantías, máquinas de estado y reconciliación

> **No se asume que ningún exchange tenga brackets atómicos.** La seguridad se construye
> con invariantes verificados de forma continua, protección **nativa** en el exchange y
> reconciliación contra la verdad del exchange. Decisión: [ADR-0002](../decisiones/ADR-0002-garantias-ejecucion-futuros.md).

---

## 1. Qué garantizamos y qué no

**Garantizamos** (con ventanas de tiempo medidas):
- Toda posición gestionada queda cubierta por un stop nativo del exchange dentro de
  `T_exp` (default 3 s) de cada aumento de tamaño, o se cierra.
- Ninguna orden de salida puede abrir posición en sentido contrario.
- Ningún comando se reenvía a ciegas cuando su resultado es desconocido.
- Si el servidor de casa se cae, las posiciones siguen protegidas, porque la protección
  vive en el exchange.

**No garantizamos** (y la UI lo dice):
- Atomicidad entre entrada y protección: existe una **ventana de exposición** medible.
- Precio de salida del stop: en gaps hay slippage (stop-market prioriza salir).
- Comportamiento del exchange ante su propia caída o mantenimiento.

---

## 2. Estrategias de protección por capacidad del exchange

Cada adaptador declara capacidades **verificadas en testnet o demo**, no supuestas
(spikes en M6). Los valores de la tabla son **hipótesis a verificar**:

| Capacidad | Binance USDⓈ-M | Bybit v5 | BloFin | BingX | MEXC |
|---|---|---|---|---|---|
| TP/SL adjuntos a la orden de entrada (se activan con los fills) | ? | probable | probable | probable | ? |
| Stop "cerrar toda la posición" sin cantidad | probable (`closePosition`) | ? | ? | ? | ? |
| Stop reduce-only aceptado **antes** de tener posición | ? | ? | ? | ? | ? |
| Reduce-only con cantidad > posición (se recorta) | ? | ? | ? | ? | ? |
| Envío en lote atómico | improbable (los lotes no son atómicos) | ? | ? | ? | ? |
| Modificación de orden en sitio | ? | ? | ? | ? | ? |
| Entorno de pruebas | testnet | testnet | demo | demo | ? |
| Órdenes de futuros por API para retail | sí | sí | ? | ? | restringido históricamente |

Estrategias, en orden de preferencia:

- **P0 — Pre-armada**: el stop de protección se coloca **antes** de enviar la entrada
  (reduce-only por la cantidad total pretendida, o "cerrar posición"). No hay ventana de
  exposición. Requiere que el exchange acepte el stop sin posición abierta.
- **P1 — Adjunta**: TP/SL adjuntos a la orden de entrada; el exchange los activa con cada
  fill. Ventana de exposición ≈ 0, pero depende de la semántica del exchange con fills
  parciales (a verificar).
- **P2 — Post-fill**: al primer fill se coloca el stop. Ventana de exposición = latencia de
  fill → colocación del stop. Es el último recurso.

Cada saga registra qué estrategia usó y cuánto duró su exposición real.

---

## 3. Máquina de estados de una orden

```
             ┌────────────── reject ──────────────► REJECTED
             │
NEW ──► PENDING_SUBMIT ──ack──► ACCEPTED ──fill──► PARTIALLY_FILLED ──fill──► FILLED
             │                     │  (condicional) ─► TRIGGERED ─► …
             │                     ├──► PENDING_CANCEL ──► CANCELED (filled_qty final)
             │                     └──► EXPIRED
             └── timeout / error de red ──► UNKNOWN
                                              │ consulta por client_order_id
                                              ├─ existe ──► estado real
                                              └─ no existe tras T_unknown ──► reintento
                                                 con el MISMO client_order_id
```

Reglas:
- `client_order_id` determinista: `hash(saga_id, rol, secuencia)`. Un duplicado
  rechazado por el exchange significa "ya existe": se consulta, no se reintenta.
- Mientras una orden está `UNKNOWN`, **ninguna orden nueva** se envía para ese
  instrumento salvo acciones de protección.
- Los eventos del stream privado y de REST pueden llegar desordenados o duplicados: la
  proyección es idempotente por `(venue_order_id, venue_trade_id)` y nunca retrocede de
  estado terminal.

---

## 4. Máquina de estados de un trade (saga)

```
DRAFT
  └─► VALIDATED            chequeos previos: tamaño mínimo, notional, SL entre entrada y
  │                        liquidación con margen, guardarraíles, sin UNKNOWN pendiente
  └─► PREPARING_ACCOUNT    apalancamiento y modo de margen (idempotente; si falla → ABORTED)
  └─► ARMING_PROTECTION    sólo P0: stop pre-armado aceptado → sigue; si falla → cae a P1/P2
  └─► ENTRY_PENDING        entrada(s) enviadas
  └─► ENTRY_WORKING        entradas aceptadas, sin fills
        │
        ├─ fill ─► ¿qty protegida ≥ |posición|?
        │            sí ─► PROTECTED (entrada parcial o completa)
        │            no ─► EXPOSED (arranca el reloj T_exp) ─► escalera de exposición (§5)
        │                                ├─► PROTECTED
        │                                └─► EMERGENCY_FLATTEN ─► FLATTENED_SAFE
        ├─ cancelada sin fills ─► CLEANUP ─► CLOSED_NO_FILL
        │
PROTECTED ─► MANAGING      TPs, mover a break-even, trailing, ajustes del usuario
  └─► CLOSING              SL o último TP ejecutándose, o cierre manual
  └─► CLOSED               posición 0, sin órdenes residuales (INV-4)

Desde cualquier estado:
  ─► UNCERTAIN             se perdió el estado (WS caído, UNKNOWN, reinicio)
       └─► RECONCILING ─► estado recalculado desde la verdad del exchange
  ─► EXTERNALLY_MODIFIED   la posición cambió sin órdenes nuestras (app del exchange,
                           liquidación, ADL) ─► reconciliar ─► re-proteger o cerrar saga
```

Reglas de interacción entre órdenes del mismo trade:
- **SL ejecutado con entrada todavía trabajando** → se cancela el resto de la entrada
  (si no, el precio vuelve y re-abre exposición).
- **Primer TP ejecutado con entrada trabajando** → por defecto se cancela el resto de la
  entrada (configurable por plantilla).
- **Entrada parcial y el usuario cancela el resto** → los TPs se re-dimensionan sobre lo
  realmente llenado; el stop sigue cubriendo (reduce-only por exceso es seguro si el
  exchange lo recorta; si no, se re-dimensiona *make-before-break*).
- **Mover el stop** (BE, trailing, manual) → *make-before-break*: se coloca el nuevo, se
  confirma aceptado, recién ahí se cancela el viejo. La doble protección momentánea es
  segura porque ambas son reduce-only.
- SL por defecto **stop-market** con disparo por **mark price** (configurable a last).

---

## 5. Escalera de exposición

Cuando una saga entra en `EXPOSED`:

1. Reintentar la protección por el carril prioritario del limitador de tasa (backoff
   200 ms, hasta 3 intentos).
2. Probar la protección alternativa (ej. "cerrar posición" si falló reduce-only sized).
3. Si el precio **ya cruzó** el nivel del SL → cierre inmediato reduce-only a mercado.
4. Si se supera `T_exp` → **cierre de emergencia** (reduce-only a mercado), cancelación del
   resto de la entrada, alerta crítica a todos los dispositivos.

La política por defecto es cerrar. Se puede configurar "sólo alertar", con advertencia
explícita, y queda registrado en el diario.

---

## 6. Invariantes (monitor continuo + propiedades en tests)

| ID | Invariante |
|---|---|
| INV-1 | Por cada (cuenta, instrumento, lado) con posición gestionada ≠ 0: Σ cantidad de stops nativos activos ≥ \|posición\|, del lado correcto, dentro de `T_exp` de cualquier aumento. |
| INV-2 | Toda orden de salida es reduce-only (o "cerrar posición"). |
| INV-3 | El precio de disparo del SL está entre la entrada y el precio de liquidación, con margen ≥ `buffer_liq`. Se re-verifica si cambia la liquidación. |
| INV-4 | Si la posición es 0 y no hay entrada trabajando, no quedan TPs ni stops residuales después de `T_cleanup` (default 10 s). |
| INV-5 | Hay a lo sumo un proceso `execution` activo (lease en Postgres; al perderlo deja de enviar inmediatamente). |
| INV-6 | Todo comando al exchange tiene `client_order_id` determinista. |
| INV-7 | Con estado `UNCERTAIN` o una orden `UNKNOWN` en un instrumento, no se aceptan entradas nuevas en él. |

Una violación genera un evento `InvariantViolation`, una alerta y la acción correctiva
correspondiente.

---

## 7. Reconciliación

- **Fuentes**: el stream privado (rápido, puede perder mensajes) y REST (lento, verdad a
  pedido).
- **Disparadores**: arranque, reconexión o hueco del stream privado, orden `UNKNOWN`,
  periódico cada 15 s para instrumentos con sagas activas y cada 5 min global, y toda
  violación de invariante.
- **Procedimiento**: traer posiciones, órdenes abiertas (incluidas condicionales) y fills
  desde el último cursor → ingerir fills faltantes → comparar con la proyección →
  clasificar las diferencias:
  - fill faltante → se ingiere;
  - orden nuestra ausente → se resuelve por historial (llena, cancelada);
  - orden ajena → se marca como **ajena**;
  - posición que no cierra con los fills → `EXTERNALLY_MODIFIED`.
- Cada corrida produce un evento `ReconciliationRun` con el resumen de diferencias.
- **Posiciones ajenas** (abiertas desde la app del exchange): se muestran con la marca
  "sin protección gestionada". El usuario puede **adoptarlas** (comando firmado) para
  que el sistema les ponga stop.

---

## 8. Límites de tasa

Un token bucket por exchange y por peso de endpoint, con **capacidad reservada** (20%)
para protección y cancelaciones. Una entrada nueva nunca puede consumir la capacidad
reservada.

---

## 9. Alternativas descartadas

- **Cancel-on-disconnect / dead man's switch del exchange**: cancelaría también los
  stops. Sólo se considera si el exchange permite limitarlo a órdenes de entrada.
- **Stops en el servidor** (triggers locales) como protección: si se corta internet en tu
  casa, la posición queda desnuda.
- **Confiar en el lote del exchange como atómico**: los lotes se procesan orden por orden.

---

## 10. Pruebas

- **Simulador de exchange determinista** (`venue-sim`) con inyección de fallas: timeouts
  con y sin efecto, acks demorados, fills parciales arbitrarios, rechazos, mensajes
  duplicados y desordenados, huecos del stream privado, cambios externos de posición.
- **Tests por propiedades**: secuencias aleatorias de lo anterior → al final de cada
  secuencia se cumplen INV-1…INV-7, o la saga terminó en `FLATTENED_SAFE` con alerta.
- **Caos**: matar `execution` en cada estado de la saga → reiniciar → reconciliar →
  invariantes OK.
- **Runbooks de testnet/demo** por exchange con los mismos escenarios.
