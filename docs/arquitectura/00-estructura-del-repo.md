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
  bender-research               estadística, calibración y validación (M9; ADR-0012)
  bender-lab                    CLI de research: datasets, reportes HTML/JSON
apps/client/                    front TypeScript (Vite, pnpm)
  packages/chart-scene          modelo de escena (ADR-0001)
  packages/render-lwc           adaptador Lightweight Charts
  packages/ui                   componentes (ticket, diario, auditoría…)
apps/shell/                     Tauri 2 (escritorio + Android)
deploy/profiles/                perfiles de nodo (server, workstation, phone, dev-all)
tools/devtools/                 herramientas del repo (check-docs-links, …)
tools/arch-tests/               tests de arquitectura (grafo de dependencias, lints)
docs/                           plan, arquitectura, ADRs, roadmap
```

## Herramientas

Todos los chequeos se corren con `make` desde la raíz; son los mismos que la CI
(`.github/workflows/ci.yml`). `make check` corre todo.

| Área | Herramientas | Target | Qué corre |
|---|---|---|---|
| Docs | Rust (`tools/devtools`) | `make docs` | `check-docs-links` (enlaces internos) |
| Rust | toolchain fijada en `rust-toolchain.toml`, clippy, rustfmt | `make rust` | `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` |
| TypeScript | pnpm (workspace en la raíz), TypeScript 6.0, Vite, Vitest, ESLint + typescript-eslint | `make ts` | `pnpm -r lint` (incluye `tsc --noEmit`), `pnpm -r test` |
| Protobuf | buf (dependencia de pnpm; `buf.yaml` en la raíz, módulo `schema/`) | `make buf` | `buf lint` (el chequeo de cambios incompatibles contra `main` lo agrega #17) |

Notas:
- Sin Python ([ADR-0012](../decisiones/ADR-0012-sin-python.md)).
- TypeScript está fijado en `~6.0` porque typescript-eslint todavía no soporta la 7.
- Playwright se agrega cuando haga falta el primer test e2e (#48).
