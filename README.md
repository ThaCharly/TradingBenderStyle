# TradingBenderStyle

Un TradingView personal: sin ruido, con ejecución en la gráfica, un motor de mercado con
fundamento auditable y un diario de trading. Un solo usuario, local-first.

- Plan y arquitectura: [docs/PLAN.md](docs/PLAN.md)
- Primer entregable: [docs/PROTOTIPO.md](docs/PROTOTIPO.md)
- Cómo trabajar (personas y agentes): [docs/AGENTES.md](docs/AGENTES.md), [docs/RAMAS.md](docs/RAMAS.md)

## Requisitos

| Herramienta | Versión | Notas |
|---|---|---|
| Rust | la de `rust-toolchain.toml` | `rustup` la instala sola al primer `cargo` |
| Node.js | ≥ 22 | |
| pnpm | la de `packageManager` en `package.json` | `corepack enable` la activa |
| make | cualquiera | |

`buf` (Protobuf) se instala como dependencia de pnpm; no hace falta instalarlo aparte.

## Primeros pasos

```sh
make setup   # instala dependencias de TypeScript (Rust se resuelve solo con cargo)
make check   # corre exactamente lo mismo que la CI
```

Chequeos por partes: `make docs`, `make rust`, `make ts`, `make buf`.

El proyecto no usa Python ([ADR-0012](docs/decisiones/ADR-0012-sin-python.md)).

Cliente web de desarrollo: `pnpm --filter @bender/client dev` (http://localhost:5173).

## Estructura

Ver [docs/arquitectura/00-estructura-del-repo.md](docs/arquitectura/00-estructura-del-repo.md).
