# Pilar 3 — Separación absoluta entre análisis y ejecución

> Los detectores, los modelos predictivos y el LLM **nunca** tienen permisos para operar.
> Sólo el módulo de ejecución envía órdenes, y sólo cuando vos se lo pedís desde un
> dispositivo tuyo.

No es una convención de código: es una propiedad que el sistema hace **imposible** de
violar por accidente o por un bug.

---

## 1. Procesos y permisos

| Servicio | Claves de exchange | Puede crear órdenes | Lee |
|---|---|---|---|
| `marketdata` (ingesta + grabador) | no | no | datos públicos |
| `engine` (niveles, eventos, estadística) | no | no | market data |
| `ml` / research (offline) | no | no | datasets |
| `llm-gateway` (PC y celular) | no | no | API de sólo lectura |
| `api` (UI, diario, dibujos, alertas) | no | no (sólo reenvía comandos firmados) | todo, sólo lectura sobre trading |
| **`execution`** | **sí, y sólo él** | **sí** | sus propios feeds + comandos firmados |

- Cada servicio corre como **usuario del sistema distinto** (o contenedor separado). El
  archivo de claves sólo es legible por el usuario de `execution`.
- Las claves se descifran **en memoria dentro de `execution`** al arrancar (passphrase o
  TPM de la mini-PC). Ningún otro proceso las ve nunca, ni cifradas.
- Sin claves, los endpoints privados de los exchanges son inútiles: esa es la barrera
  real. Adicionalmente, firewall de salida por servicio.

---

## 2. El único camino de una orden

```
Vos (gesto en la UI, PC o celular)
  → comando firmado con la clave del dispositivo
  → api (lo reenvía, no puede modificarlo: la firma se rompería)
  → execution: verifica firma, nonce, vigencia, guardarraíles
  → exchange
```

- **Emparejamiento de dispositivos**: PC y celular generan un par de claves al
  vincularse. En el celular la clave vive en el Android Keystore (hardware) y se
  desbloquea con biometría.
- `execution` **sólo** acepta comandos firmados por dispositivos emparejados. El motor y
  el LLM no tienen clave de dispositivo: no pueden fabricar un comando válido.
- Cada orden guarda su origen: dispositivo, intent, ticket, hora. Auditable.

---

## 3. `execution` no escucha al análisis

- `execution` **no se suscribe** a eventos del motor, ni a predicciones, ni al LLM.
- Los **triggers de servidor** (órdenes que viven en tu servidor y no en el exchange) los
  definís vos, firmados, y los evalúa `execution` con **su propio feed de precios**.
  Condiciones permitidas: de precio y de cierre de vela ("si cierra 4h debajo de X").
  **No** existen condiciones del tipo "si el motor detecta un rechazo, comprar": eso queda
  fuera del diseño, a propósito.
- Los **guardarraíles** (pérdida diaria máxima, cooldown, riesgo por trade) viven dentro de
  `execution`, porque son reglas de ejecución, no de análisis.

---

## 4. El LLM

- Su registro de herramientas es **de sólo lectura por construcción**: no existe ninguna
  tool que toque órdenes. El token del `llm-gateway` no tiene el alcance `trade`.
- Puede escribir en un espacio propio del diario ("notas del asistente": resúmenes,
  revisiones), separado y marcado. Nunca modifica registros de trades ni tus notas.
- Igual en el celular: el modelo local usa el mismo token de sólo lectura.

---

## 5. La UI

- La UI muestra el análisis al lado de la gráfica y del ticket, pero **el análisis nunca
  rellena el ticket**. Si querés comprar en un nivel del motor, arrastrás la línea vos.
- No existe un botón "tomar esta sugerencia", porque no hay sugerencias.

---

## 6. Alcances de API (tokens)

`read:market`, `read:engine`, `read:account`, `read:journal`, `write:drawings`,
`write:alerts`, `write:journal_notes`, `trade:command` (este último sólo dispositivos
emparejados con firma).

---

## 7. Botón rojo

Desde cualquier dispositivo emparejado: **cancelar todo** y **cerrar todo** (por exchange
o global). Es una orden más, firmada, que `execution` procesa con prioridad.
