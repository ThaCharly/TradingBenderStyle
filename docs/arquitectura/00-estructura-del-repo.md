# Estructura del repositorio

Monorepo. Los nombres de esta página son los que usan los issues: si un issue dice
"crate `bender-kernel`", es la carpeta `crates/bender-kernel`.

```
schema/                         IDL Protobuf (paquete bender.v1), gestionado con buf
crates/                         workspace de Rust
  bender-types                  tipos generados + Decimal, ids (ULID), tiempo (ns UTC)
  bender-kernel                 núcleo temporal (doc 05): sobre, orden, reloj, velas, as_of
  bender-store                  segmentos Parquet, hashes, manifiestos, checkpoints
  bender-runner                 Runner con modos live/replay/backtest/dataset
  bender-venue-api              trait Venue, matriz de capacidades, suite de conformidad
  bender-venue-sim              simulador determinista de exchange con inyección de fallas
  bender-venue-paper            venue paper (fills sobre datos reales)
  bender-venue-binance          adaptador Binance (spot + USDⓈ-M)
  bender-venue-blofin           adaptador BloFin
  bender-venue-bingx            adaptador BingX
  bender-venue-bybit            adaptador Bybit
  bender-venue-mexc             adaptador MEXC
  bender-recorder               binario: grabador de datos públicos
  bender-backfill               binario: importadores de dumps históricos
  bender-engine                 motor L1–L4 (features, niveles, eventos, confluencias)
  bender-execution              binario: OMS, sagas, reconciliación, keystore, guardarraíles
  bender-commands-client        cliente de comandos firmados (lo usan sólo las UIs)
  bender-api                    binario: gateway para clientes (auth, reenvío de comandos)
  bender-query                  servicio de consultas de sólo lectura
  bender-journal                servicio de diario (incluye assistant_notes)
  bender-notify                 notificaciones (push, Telegram)
  bender-llm-gateway            binario: servidor de herramientas del LLM
  bender-py                     bindings PyO3 del kernel/engine/runner
python/bender_research/         estadística, calibración, ML (uv)
apps/client/                    front TypeScript (Vite, pnpm)
  packages/chart-scene          modelo de escena (ADR-0001)
  packages/render-lwc           adaptador Lightweight Charts
  packages/ui                   componentes (ticket, diario, auditoría…)
apps/shell/                     Tauri 2 (escritorio + Android)
deploy/profiles/                perfiles de nodo (server, workstation, phone, dev-all)
tools/arch-tests/               tests de arquitectura (grafo de dependencias, lints)
docs/                           plan, arquitectura, ADRs, roadmap
```

## Herramientas

| Lenguaje | Herramienta | Comando de verificación |
|---|---|---|
| Rust (stable) | cargo, clippy, rustfmt | `cargo fmt --check && cargo clippy --workspace -- -D warnings && cargo test --workspace` |
| TypeScript | pnpm, Vite, Vitest, Playwright | `pnpm -r lint && pnpm -r test` |
| Python | uv, ruff, pytest | `uv run ruff check && uv run pytest` |
| Protobuf | buf | `buf lint && buf breaking --against .git#branch=main` |
