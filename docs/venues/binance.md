# Binance (spot + USDⓈ-M perps)

Primer venue del grabador (#32). Código: `crates/bender-venue-binance` (normalizador puro y
libro local) y `crates/bender-recorder` (binario). Sólo datos públicos: el grabador no
activa la feature `private` y no tiene acceso a claves (Pilar 3).

## Conexiones

| Conexión | URL | Streams |
|---|---|---|
| spot | `wss://stream.binance.com:9443/stream` | todos |
| USDⓈ-M `/public` | `wss://fstream.binance.com/public/stream` | `depth`, `bookTicker` |
| USDⓈ-M `/market` | `wss://fstream.binance.com/market/stream` | `aggTrade`, `markPrice`, `forceOrder` |

Binance separó los streams de futuros en dos rutas. La ruta sin sufijo
(`wss://fstream.binance.com/stream`) **ya no entrega trades ni mark price**, sólo el libro:
verificado con capturas reales (2026-10, `bender-recorder capture`). Cada conexión tiene
sus propios huecos: si se cae sólo `/market`, el libro no queda `incomplete`.

## Qué se graba

Universo por defecto (`crates/bender-recorder/recorder.example.toml`): BTCUSDT, ETHUSDT,
BTCUSDC y ETHUSDC en spot y en perps USDⓈ-M. Libro L2 (top-20, ADR-0009) para los USDT;
el resto, top-of-book.

| Stream / endpoint | Fuente canónica | `source_seq` | `ts_event` |
|---|---|---|---|
| `<s>@aggTrade` | `…:trades` (`Trade`) | id de trade agregado (`a`) | `T` |
| `<s>@depth@100ms` + `GET depth` | `…:book` (`BookSnapshot`, `BookDelta` del top-N) | `u` / `lastUpdateId` | `E` (snapshot de spot: recepción) |
| `<s>@bookTicker` | `…:top_of_book` | `u` | `E` (spot no lo trae: recepción) |
| `<s>@markPrice@1s` (perps) | `…:mark_price`, `…:index_price`, `…:funding` | — | `E` |
| `<s>@forceOrder` (perps) | `…:liquidations` | — | `T` de la orden |
| `GET /fapi/v1/openInterest` cada 60 s | `…:open_interest` | — | `time` |
| latido cada 100 ms | `CLOCK` | — | reloj de pared |

- Sobre: `ts_received` = reloj de la máquina al recibir; `ts_available = ts_received`
  (si dos eventos de una fuente llegan en el mismo instante con la secuencia invertida, el
  segundo va 1 ns después: dentro de una fuente el orden K nunca retrocede).
  `ingest_seq` es propio de cada fuente y continúa entre reinicios.
- Agresor: `m = true` (el comprador es maker) → agresor vendedor.
- Liquidación: precio medio (`ap`) y cantidad ejecutada (`z`); `side` es el de la orden de
  liquidación (`SELL` = se liquidó un largo).
- Libro: snapshot REST + deltas con verificación de secuencia (spot: `U ≤ L+1 ≤ u` y
  después `U = u_anterior + 1`; perps: `U ≤ L ≤ u` o `pu = L`, y después
  `pu = u_anterior`). Se graban sólo los cambios dentro del top-N (un nivel que sale del
  top-N aparece con cantidad 0) y un snapshot del top-N cada 60 s.
- Zona cruda: cada mensaje tal cual, en la fuente `BINANCE:<mercado>:<símbolo>:ws:<stream>`
  o `…:rest:<endpoint>` (`ws:other` para lo que no se reconoce).
- Normalizador versionado: `binance-1` (se graba en cada segmento normalizado).

## Recuperabilidad (ADR-0011)

| Tipo de dato | Recuperable | Cómo | Resultado típico |
|---|---|---|---|
| Trades | **sí** | `GET aggTrades?fromId=` desde el último id visto hasta el primero visto después del hueco (ids continuos) | `full` si la continuidad de ids se restablece; `partial` si el proveedor devuelve sólo parte o se llega al tope de páginas |
| Libro L2 | **no** | snapshot nuevo para resincronizar (`RESNAPSHOT`) | `none`: el intervalo queda `incomplete` |
| Top-of-book | no | — | `none` |
| Mark / index / funding | no en P0 | (endpoints históricos con menor resolución; fuera de alcance de #32) | `none` |
| Liquidaciones | no | Binance no tiene histórico de `forceOrder` | `none` |
| Open interest | no en P0 | (`openInterestHist`, resolución ≥ 5 min) | sin hueco: se consulta por polling |

## Huecos

Un `Gap` se emite cuando la fuente vuelve, que es cuando se conocen sus dos bordes
(`to_ts` = primer dato visto después):

| Causa | `reason` | Fuentes | `from_ts` → `to_ts` |
|---|---|---|---|
| Se cayó la conexión | `DISCONNECTED` | las que viajan por esa conexión | último mensaje de la conexión → reconexión (el libro: → resincronización) |
| Silencio de más de 10 s sin cierre | `STALE` | ídem (el grabador cierra y reconecta) | ídem |
| Salto de ids de trade | `SEQUENCE_BREAK` | trades | trade anterior → trade que salta |
| Secuencia del libro rota | `SEQUENCE_BREAK` | libro | último delta válido → resincronización |
| El grabador estuvo detenido | `PROCESS_DOWN` | las que ya tenían datos | último latido grabado → primera conexión |

La recuperación nunca borra el `Gap`: los trades recuperados llegan con
`provenance = recovered` y un `ts_available` posterior, seguidos de un `GapRecovery`.

## Límites de Binance

- REST: presupuesto de peso por minuto y por mercado (por defecto la mitad de lo que
  permite Binance: 3000 spot, 1200 perps). Pesos usados: snapshot `limit=1000` 50 (spot) /
  20 (perps), `aggTrades` 4 / 20, `openInterest` 1. La recuperación usa como mucho el 50 %
  del presupuesto y va después de los snapshots. Un 429/418 pausa el mercado lo que diga
  `Retry-After`.
- WS: una conexión por ruta (spot; USDⓈ-M `/public` y `/market`) con todos los streams en
  la URL; reconexión con backoff exponencial de 1 s a 60 s. Binance corta cada conexión a las 24 h: eso deja un hueco
  corto (trades recuperados; libro resincronizado).

## Operación

```bash
cargo run --release -p bender-recorder -- run --store data/store
```

`--config <archivo.toml>` cambia el universo o los parámetros (ejemplo con los defaults en
`crates/bender-recorder/recorder.example.toml`). Ctrl+C cierra los segmentos; si el proceso
muere, el store los recupera al abrirse y la próxima corrida deja un `Gap` de apagado.

```bash
cargo run --release -p bender-recorder -- report --store data/store
```

Reporte en Markdown: duración, disco por zona y tipo de dato (trades / L2 / otros) con la
proyección por día, huecos por motivo y, por fuente, filas, tamaño, huecos y
recuperaciones.

### Fixtures reales del normalizador

```bash
cargo run -p bender-recorder -- capture --out crates/bender-venue-binance/tests/fixtures/reales
```

Graba, por mercado, el primer mensaje de cada stream, un snapshot del libro con un delta
que engancha, una página de `aggTrades` y el open interest, más el `.esperado` de cada uno
según el normalizador actual. Al terminar imprime, por mercado, cuántos mensajes llegaron
de cada stream (sirve para detectar cambios de rutas de Binance como el de `/public` y
`/market`). Revisar los `.esperado` a mano antes de commitear; los verifica
`cargo test -p bender-venue-binance --test fixtures`.
