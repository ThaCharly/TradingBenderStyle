# Consultas con DuckDB sobre el store

Los segmentos de `bender-store` son Parquet comunes. DuckDB
([ADR-0007](../decisiones/ADR-0007-almacenamiento-minimo.md)) los lee directo, sin
importar nada.

## Dónde están

```
<raíz>/manifest.jsonl                                   índice de segmentos cerrados
<raíz>/normalized/<fuente>/<AAAA-MM-DD>/<seq>-<hash>.parquet
<raíz>/raw/<fuente>/<AAAA-MM-DD>/<seq>-<hash>.parquet
```

`<fuente>` es la clave canónica con `:` cambiado por `~`, por ejemplo
`BINANCE~PERP_LINEAR~BTCUSDT~trades`. Dentro de cada archivo, la columna `source` tiene la
clave original (`BINANCE:PERP_LINEAR:BTCUSDT:trades`).

| Zona | Columnas |
|---|---|
| `normalized` | `source`, `ts_event`, `ts_received` (puede ser nulo), `ts_available`, `source_seq` (puede ser nulo), `ingest_seq`, `provenance` (`live`, `recovered`, `backfill`, `synthetic`), `payload_kind` (`trade`, `book_delta`, `heartbeat`, `gap`…), `normalizer_version`, `envelope` (el `bender.v1.Envelope` en Protobuf) |
| `raw` | `source`, `ts_received`, `ingest_seq`, `payload` (el mensaje tal cual llegó) |

Los tiempos son enteros en ns UTC. Para verlos como fecha:
`make_timestamp(ts_available // 1000)`.

Los segmentos abiertos (`open.wal`) no son Parquet, así que no aparecen hasta que se
cierran.

## Ejemplos

En las consultas, `datos/` es la raíz del store. Un store de prueba se genera con
`cargo run -p bender-store --example write-sample -- datos`. El test de
`tools/store-sql` corre estas mismas consultas contra ese store, con DuckDB, y verifica el
resultado.

Trades por fuente y día UTC:

```sql
SELECT source,
       strftime(make_timestamp(ts_available // 1000), '%Y-%m-%d') AS dia,
       count(*) AS trades,
       min(ingest_seq) AS primero,
       max(ingest_seq) AS ultimo
FROM read_parquet('datos/normalized/*/*/*.parquet')
WHERE payload_kind = 'trade'
GROUP BY ALL
ORDER BY ALL;
```

Mensajes y bytes crudos por fuente:

```sql
SELECT source, count(*) AS mensajes, sum(octet_length(payload)) AS bytes
FROM read_parquet('datos/raw/*/*/*.parquet')
GROUP BY source
ORDER BY source;
```

Para ver el contenido de un trade (precio, cantidad) hay que decodificar `envelope` con los
tipos generados (`bender-types` en Rust, `@bender/types` en TS). En SQL sólo están las
columnas de arriba.
