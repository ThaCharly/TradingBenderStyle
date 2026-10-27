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
| Python | ≥ 3.12 | |
| uv | reciente | gestiona el entorno de `python/` |
| make | cualquiera | |

`buf` (Protobuf) se instala como dependencia de pnpm; no hace falta instalarlo aparte.

## Primeros pasos

```sh
make setup   # instala dependencias de TS y Python
make check   # corre exactamente lo mismo que la CI
```

Chequeos por partes: `make docs`, `make rust`, `make ts`, `make python`, `make buf`.

Cliente web de desarrollo: `pnpm --filter @bender/client dev` (http://localhost:5173).

## Estructura

Ver [docs/arquitectura/00-estructura-del-repo.md](docs/arquitectura/00-estructura-del-repo.md).
