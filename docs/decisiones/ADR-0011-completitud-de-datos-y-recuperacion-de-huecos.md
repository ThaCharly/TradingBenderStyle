# ADR-0011 — Completitud de datos y recuperación de huecos

- Estado: **aceptada**
- Fecha: 2026-10-27
- Afecta: Pilar 1 (eventos `Gap`), Pilar 2 (bitemporalidad), grabador (#32), núcleo (#24),
  consultas (#46), gráfica (#48)

## Contexto

El grabador se va a desconectar. El plan ya registraba los huecos como eventos `Gap`, pero
no distinguía entre un intervalo grabado entero, uno rellenado después y uno que quedó
roto. Tampoco obligaba a intentar recuperar lo perdido. Mezclar esos casos contamina
estadísticas y backtests sin que se note.

## Decisión

### 1. Tres estados de completitud, explícitos y por intervalo

Para cada fuente (venue + instrumento + tipo de dato) y cada intervalo de tiempo:

| Estado | Significado |
|---|---|
| `complete` | grabado en vivo sin interrupción (o con continuidad verificada, p. ej. ids de trade consecutivos) |
| `recovered` | hubo un hueco y se rellenó **después** desde el proveedor (REST/histórico); los datos son los del proveedor pero no se vieron en vivo |
| `incomplete` | hubo un hueco y no se pudo rellenar, o se rellenó sólo en parte |

Además, cada evento lleva su procedencia en el sobre (`provenance`):
`live` (visto en vivo), `recovered` (rellenado tras un hueco), `backfill` (importación
histórica masiva, p. ej. dumps) y `synthetic` (latidos sintéticos).

### 2. La discontinuidad nunca se oculta

- Toda interrupción emite un `Gap { gap_id, source, from, to, reason }` en el momento en
  que se detecta, con `ts_available` del momento de la detección.
- La recuperación **no borra ni reemplaza** el `Gap`. Agrega datos con
  `provenance = recovered` y un `ts_available` posterior (el instante en que se
  recuperaron), más un evento
  `GapRecovery { gap_id, recovered_from, recovered_to, method, result: full | partial | none }`.
- Por bitemporalidad, una consulta `as_of` anterior a la recuperación ve el hueco tal como
  se vivió; una posterior ve los datos recuperados marcados como tales.

### 3. Primero intentar recuperar

Al reconectar, el grabador intenta recuperar el intervalo perdido **si el proveedor lo
permite**, antes de declararlo `incomplete`:

| Tipo de dato | Recuperabilidad típica | Cómo |
|---|---|---|
| Trades | alta cuando hay ids secuenciales (Binance) | pedir por rango de ids/tiempo; la continuidad de ids prueba que está `full` |
| Velas oficiales | alta | sólo para validar; nunca reemplazan trades |
| Mark/index, funding | media | endpoints históricos, a menudo con menor resolución → `partial` |
| Open interest | baja (historia corta y de baja resolución) | `partial` o `none` |
| Libro L2 | **ninguna** | snapshot nuevo para resincronizar; el intervalo queda `incomplete` |

La capacidad de recuperación es por venue y tipo de dato, configurable y documentada en
`docs/venues/<venue>.md`. Los límites de tasa del proveedor se respetan (la recuperación
corre con menor prioridad que la grabación en vivo).

### 4. Consumidores

- `bender-kernel` expone `coverage(source, rango, as_of)` → lista de intervalos con su
  estado.
- `bender-query` devuelve la cobertura junto con las velas; cada vela informa si su
  intervalo fue `complete`, `recovered` o `incomplete`.
- La gráfica dibuja los intervalos `incomplete` (y opcionalmente los `recovered`) de forma
  visible; no interpola.
- La estadística (M9) filtra o marca casos según la completitud de sus datos.

## Alternativas descartadas

- **Rellenar y borrar el `Gap`**: oculta la discontinuidad y rompe la reproducibilidad de
  lo que se sabía en vivo.
- **Marcar todo lo rellenado como `live`**: mezcla datos vistos en vivo con datos
  reconstruidos; los tiempos de llegada serían falsos.
- **No recuperar nada**: pierde trades que el proveedor sí puede devolver.
- **Interpolar velas faltantes**: inventa datos.

## Consecuencias

- El esquema (#17) agrega `GapRecovery`, el valor `recovered` en `provenance` y el tipo
  de cobertura.
- El grabador (#32) implementa la recuperación de trades de Binance por ids dentro de P0.
  Las demás recuperaciones (funding, mark, OI) siguen la misma interfaz cuando haga falta.
