# Núcleo temporal (`bender-kernel`)

> Una sola librería define **todas** las reglas de tiempo del sistema. El análisis en
> vivo, el replay, el backtest y la generación de datasets de entrenamiento son **el mismo
> código** con distinta fuente de eventos. Decisión: [ADR-0004](../decisiones/ADR-0004-nucleo-temporal-primero.md).

Este documento es la especificación normativa. Si el código y este documento difieren,
uno de los dos tiene un bug.

---

## 1. Sobre de evento

Todo lo que entra al núcleo (mercado, acciones del usuario, latidos de reloj) viaja en el
mismo sobre:

```
Envelope
  ts_event       i64 ns UTC   cuándo pasó (según la fuente)
  ts_received    i64 ns UTC   cuándo llegó a nuestro servidor (sólo grabado en vivo)
  ts_available   i64 ns UTC   cuándo el núcleo puede usarlo
  source         SourceId     ej. BINANCE:PERP_LINEAR:BTCUSDT:trades, USER:drawings, CLOCK
  source_seq     u64?         secuencia de la fuente, si existe
  ingest_seq     u64          secuencia propia, monótona por fuente
  provenance     live | backfill | synthetic
  payload        MarketEvent | UserEvent | Heartbeat | Gap
```

## 2. Orden total

Clave de orden: `K = (ts_available, source_rank, source_seq, ingest_seq)`.

- `source_rank` es una tabla fija y versionada (ej. `CLOCK` < `USER` < venues por orden
  alfabético). Desempata eventos simultáneos de forma estable.
- Ningún componente procesa eventos fuera de este orden.

## 3. El reloj

- El tiempo del núcleo es el `ts_available` del evento que se está procesando. **Ningún
  crate del núcleo ni del motor lee el reloj del sistema** (lint de CI con
  `clippy::disallowed_methods` sobre `SystemTime::now`, `Instant::now`, `chrono::Utc::now`).
- Para que las condiciones por tiempo funcionen sin trades (ej. "cierre de vela de 4h"),
  la fuente en vivo emite **latidos** (`Heartbeat`) cada `G` = 100 ms con
  `ts_available` = reloj de pared. **Los latidos se graban** como cualquier evento, así
  el replay ve exactamente los mismos.
- En histórico reconstruido, los latidos se sintetizan en la grilla `G`
  (`provenance = synthetic`).

## 4. Disponibilidad en datos reconstruidos

| Dato reconstruido | `ts_available` |
|---|---|
| trade de dump público | `ts_event + δ_trade` (default 500 ms) |
| vela de terceros | `open_time + duración + δ_bar` (default 2 s) |
| funding / OI de API histórica | `ts_event + δ_stat` (default 5 s) |

Los δ son parte de la configuración versionada y se registran en el manifiesto.

## 5. Marca de agua y datos tardíos

- Por fuente: `watermark = max(ts_event visto) − L`, con `L` = demora permitida
  (default 2 s para trades).
- Un evento con `ts_event` menor que el fin de una vela ya cerrada es **tardío**: se
  aplica igual, pero genera un `BarRevision` (revisión n+1) con su propio `ts_available`.
- El motor, por defecto, usa la **primera versión cerrada** de cada vela (la que se conocía
  en ese momento). Las revisiones existen y son consultables `as_of`.

## 6. Velas

- Alineadas al epoch UTC por temporalidad. Semana: lunes 00:00 UTC. Mes y año:
  calendario UTC. Personalizadas: múltiplos de 1 s alineados al epoch.
- Una vela de `[t, t+d)` **cierra** cuando la marca de agua de su fuente supera `t+d`, o
  cuando llega un latido con `ts_available ≥ t + d + L`, lo que ocurra primero.
- Antes de cerrar, la vela existe como **en formación** (`forming`) y se actualiza.
- Campos: OHLC, volumen base, volumen quote, volumen comprador/vendedor (agresor),
  cantidad de trades, primer y último `ts_event`, revisión, fuentes incluidas.
- **Compuestas multi-venue**: cierran cuando todas las fuentes incluidas superaron
  `t+d` o tras una espera máxima `M` (default 5 s); las fuentes que faltaron quedan
  registradas como incompletas en la vela.

## 7. Acciones del usuario

Dibujos, niveles marcados, definiciones de alerta y cambios de configuración entran al
log como `UserEvent` con `ts_available` = recepción en el servidor. Así "qué tenía
dibujado el 10 de mayo" es una consulta `as_of`.

## 8. Consulta `as_of(T)`

`as_of(T)` = estado obtenido de procesar, en orden `K`, todos los eventos con
`ts_available ≤ T`, partiendo del checkpoint más cercano anterior a `T`.

## 9. Modos de ejecución (un solo `Runner`)

| Modo | Fuente | Ritmo | Salida |
|---|---|---|---|
| `live` | streams en vivo + grabación simultánea | tiempo real | estado, eventos del motor, alertas |
| `replay` | log grabado | velocidad elegida (×1, ×10, paso a paso) | idéntica a `live` |
| `backtest` | log grabado o reconstruido | lo más rápido posible | idéntica a `live` + métricas |
| `dataset` | log grabado o reconstruido | lo más rápido posible | filas de features + etiquetas |

En modo `dataset`, las features se calculan con una vista que **sólo** puede leer el
pasado (`PastView`). Las etiquetas las calcula un `Labeler` separado que recibe
explícitamente una `FutureWindow`. El sistema de tipos impide que una feature reciba una
`FutureWindow`.

## 10. Checkpoints y manifiestos

- Checkpoint diario (00:00 UTC) del estado completo, serializado de forma canónica, con
  hash. Es caché: se verifica re-corriendo desde cero.
- Manifiesto de corrida: hashes de los segmentos de datos, commit del código, hash de la
  configuración (incluidos δ, `L`, `G`, `M` y versiones de definiciones de eventos) y hash
  de la salida.

## 11. Pruebas normativas

1. **Incremental = batch**: mismo hash de salida.
2. **Doble corrida**: mismo hash en dos máquinas (x86 servidor y PC).
3. **Vivo = replay**: el test dorado nocturno replaya las últimas 24 h grabadas y compara
   las salidas emitidas en vivo.
4. **Canario de futuro**: un evento con `ts_available` futuro no puede afectar ninguna
   salida anterior.
5. **Checkpoint = desde cero**: estado en `T` desde checkpoint = estado en `T` desde el
   inicio.
