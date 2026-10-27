# Pilar 2 — Motor histórico reproducible

> Volver al 10 de mayo de 2026 a las 10:00 y reconstruir **exactamente** qué información
> estaba disponible en ese momento: datos, niveles, eventos, estado de la cuenta y
> configuración. Sin contaminación del futuro.

---

## 1. La definición

El motor es una **función pura**:

```
estado(T) = f( eventos con ts_available ≤ T  (en orden total),
               configuración vigente,
               versión del código )
```

Si la entrada es la misma, la salida es idéntica bit a bit. En una máquina o en otra,
hoy o dentro de dos años.

---

## 2. Datos: tres zonas

| Zona | Qué guarda | Mutabilidad |
|---|---|---|
| **Cruda** | cada mensaje WS/REST tal cual llegó + `ts_received` | append-only, comprimida, con hash por bloque |
| **Normalizada** | modelo canónico (Pilar 1) derivado de la cruda | re-derivable; se regenera si cambia el normalizador (nueva versión, la vieja no se borra) |
| **Derivada** | velas, perfiles, features, niveles, eventos | caché; siempre recalculable |

La zona cruda es la verdad. Todo lo demás se puede tirar y reconstruir.

**Retención** ([ADR-0009](../decisiones/ADR-0009-presupuesto-de-disco.md)): la zona cruda
se conserva 90 días. Pasado ese plazo, la zona normalizada (validada a diario por el
control de calidad) pasa a ser la verdad de largo plazo.

---

## 3. Bitemporalidad

Cada dato tiene **cuándo pasó** (`ts_event`) y **cuándo lo supimos** (`ts_available`).
La consulta fundamental es `as_of(T)`: todo lo que se sabía en `T`.

- **Correcciones**: nunca se sobrescribe. Si un exchange corrige un dato o rellenamos un
  hueco después, entra como registro nuevo con su propio `ts_available`. Una consulta
  `as_of` anterior a la corrección ve el dato sin corregir, **como lo viste en ese
  momento**.
- **Especificaciones de instrumentos**: también con vigencia (Pilar 1, §3).
- **Configuración del motor y dibujos tuyos**: versionados con el tiempo. "¿Qué niveles
  tenía marcados yo el 10 de mayo?" tiene respuesta.

---

## 4. Histórico grabado vs reconstruido

Hay dos tipos de historia, y se distinguen siempre:

- **Grabada** (`provenance = live`): lo que capturó nuestro grabador en vivo.
  `ts_available` es real. Es la historia perfecta.
- **Reconstruida** (`provenance = backfill`): dumps públicos (Binance, Bybit),
  Bitstamp/Coinbase para BTCUSD desde 2011, etc. No sabemos cuándo habría llegado cada
  dato, así que se estima de forma conservadora: trades → `ts_event + δ`; velas →
  disponibles recién al cierre `+ δ`. δ configurable (ej. 500 ms).

**Consecuencia práctica: el grabador en vivo arranca en la Fase 1, antes que casi
cualquier otra cosa.** Cada día que corre es un día de historia perfecta, de todos los
venues y mercados (spot + perps de Binance, BloFin, BingX, MEXC, Bybit), aunque todavía
no operes en ellos. BloFin, BingX y MEXC tienen poco histórico público descargable: para
ellos, la historia empieza el día que prendamos el grabador.

---

## 5. Huecos

Si la conexión se corta, se emite un evento `Gap`. El motor ve el hueco igual en vivo que
en replay. Si después rellenamos el hueco con datos REST, entran como datos tardíos (con
`ts_available` posterior). Así el replay reproduce lo que el motor realmente sabía, no lo
que sabríamos con el diario del lunes.

Al reconectar, el grabador **intenta primero recuperar** lo perdido si el proveedor lo
permite. Los datos recuperados llevan `provenance = recovered`, el `Gap` nunca se borra, y
cada intervalo queda clasificado como `complete`, `recovered` o `incomplete`
([ADR-0011](../decisiones/ADR-0011-completitud-de-datos-y-recuperacion-de-huecos.md)).

---

## 6. Reglas de determinismo (para el código del motor)

- **El reloj es una entrada.** El motor nunca lee la hora del sistema; recibe el tiempo
  de los eventos.
- **Orden total** de eventos: `(ts_available, venue, venue_seq, ingest_seq)`.
- Decimales de punto fijo para precios/cantidades. Las features en float se calculan en
  orden fijo (nada de reducciones paralelas no deterministas dentro del motor).
- Nada de iterar mapas sin orden definido.
- Aleatoriedad sólo con semilla registrada.
- **Una sola implementación** del motor (librería Rust) usada en vivo, en replay, en
  backtest y en el pipeline de entrenamiento (vía bindings a Python).

---

## 7. Checkpoints y manifiestos

- **Checkpoints**: snapshots del estado del motor (por ejemplo, cada día UTC) con hash.
  Para ir al 10/05/2026 10:00 se carga el checkpoint del 10/05 00:00 y se re-ejecuta
  hasta las 10:00. Son caché: siempre verificables re-corriendo desde cero.
- **Manifiesto de corrida**: `{ hashes de datasets, commit del código, hash de config,
  versión de definiciones de eventos }` → hash de salida. **Todo resultado** (nivel,
  evento, alerta, probabilidad, predicción de modelo) referencia su manifiesto. Con eso
  cualquier número se puede recalcular y verificar.

---

## 8. La máquina del tiempo (UI)

Elegís una fecha y hora y la app entera se pone en ese momento:
- la gráfica muestra sólo datos `≤ T`, con las velas parciales como estaban;
- niveles, rangos y eventos del motor tal como existían en `T`;
- tus dibujos y alertas vigentes en `T`;
- estado de cuenta y posiciones en `T`;
- desde ahí podés darle **play** (replay) con cuenta paper.

---

## 9. Pruebas que custodian el pilar

1. **Incremental = batch**: correr el motor evento a evento y correrlo en lote sobre el
   mismo período da el mismo hash.
2. **Doble corrida**: dos ejecuciones idénticas → mismo hash (en máquinas distintas).
3. **Vivo = replay**: las alertas y eventos emitidos en vivo la semana pasada, al
   replayar la grabación cruda de esa semana, salen idénticos (test dorado nocturno).
4. **Canario de futuro**: se inyecta en los datos un evento marcado con
   `ts_available` futuro; si cualquier salida del motor en `T` depende de él, falla.

---

## 10. Entrenamiento sin contaminación

- **Features**: siempre `as_of(t)` del evento.
- **Etiquetas**: miran explícitamente hacia adelante (ventana de horizonte), y sólo ahí.
- **Splits**: walk-forward (entrenar en pasado, evaluar en futuro), con **purga** de casos
  cuyo horizonte se solapa con el período de evaluación y **embargo** posterior.
- Cada modelo entrenado guarda su manifiesto (datos, período, features, código).
