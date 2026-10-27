# ADR-0007 — Almacenamiento mínimo: Parquet + DuckDB + Postgres; ClickHouse diferido

- Estado: **aceptada** (simplifica el plan v2)
- Fecha: 2026-10-27

## Contexto

El plan v2 proponía Parquet + ClickHouse + Postgres en una mini-PC. Para un solo usuario,
ClickHouse agrega un servicio con consumo de memoria relevante y operación propia, sin que
hayamos medido que haga falta. Las velas pre-agregadas por temporalidad son chicas; lo
pesado (trades crudos) se lee sobre todo en procesos por lotes.

## Decisión

- **Zona cruda y normalizada**: segmentos Parquet append-only, direccionables por hash.
- **Consultas analíticas y por lotes**: DuckDB embebido sobre Parquet.
- **Velas para la gráfica**: rollups materializados en Parquet por temporalidad, más una
  caché en memoria en `query` para la ventana visible.
- **Estado transaccional** (órdenes, eventos de trading, ledger, dibujos, alertas, diario,
  leases, cola de trabajos): Postgres.

## Criterio para incorporar ClickHouse (o QuestDB)

Si `query` no logra servir 5.000 velas de cualquier temporalidad en ≤ 50 ms (p95) en la
mini-PC, o si las consultas ad hoc sobre trades en vivo se vuelven necesarias desde la UI.

## Alternativas descartadas

- **ClickHouse desde el inicio**: operación y memoria sin necesidad medida.
- **TimescaleDB para todo**: mezcla datos crudos voluminosos con el estado transaccional
  en la misma base y complica los respaldos.
