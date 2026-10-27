# ADR-0012 — Sin Python: research, estadística y herramientas en Rust

- Estado: **aceptada**
- Fecha: 2026-10-27
- Reemplaza en parte: la fila "Research, ML: Python" del stack (PLAN §10), el issue #31
  (bindings PyO3) y el proyecto `python/` creado en #2

## Contexto

El usuario prefiere no usar Python. En el plan, Python cumplía tres papeles:
1. research y estadística (M9: datasets, modelos base, walk-forward, calibración, métricas);
2. ML (M12: gradient boosting, redes chicas) y el afinado del LLM (M11-07, LoRA);
3. scripts de herramientas (verificador de enlaces) y los bindings PyO3 (#31).

El motor ya es Rust y corre igual en vivo, replay, backtest y dataset. Tener la
estadística en el mismo lenguaje **elimina una frontera** (bindings, conversión de
decimales, dos toolchains) y refuerza el Pilar 2: todo el cálculo temporal y estadístico
vive en un solo lugar.

## Decisión

1. **Producto y herramientas: sin Python.** Rust para todo lo de backend, research y
   herramientas; TypeScript para la UI.
2. **Research y estadística en Rust** (crate `bender-research` + CLI `bender-lab`):
   - datos: el Runner en modo `dataset` escribe Parquet; se lee con Polars (Rust) o DuckDB
     (crate `duckdb`) para consultas SQL;
   - estadística: `statrs` (distribuciones), implementación propia de lo específico
     (beta-binomial jerárquico, bootstrap por bloques, purga/embargo, métricas de Murphy,
     pendiente/intercepto de calibración, Platt e isotónica, que son algoritmos chicos y
     testeables);
   - reportes: HTML/JSON generados por `bender-lab`, y paneles en la propia UI (M9-09).
3. **Exploración interactiva**: DuckDB CLI (SQL) sobre los Parquet, más los paneles de la
   UI. Sin notebooks de Python.
4. **ML (M12)**: a evaluar al refinar M12, en Rust:
   - gradient boosting: LightGBM vía FFI (crate de bindings) o una implementación pura en
     Rust;
   - logística y modelos lineales: `linfa`;
   - redes chicas: `candle` o `burn`.
5. **Excepción posible: afinado LoRA del LLM (M11-07).** El ecosistema maduro de
   fine-tuning es Python. Si al llegar a M11 no hay una alternativa viable (llama.cpp u
   otra herramienta sin Python), se usa un **pipeline aislado y descartable** (contenedor),
   fuera del repo principal o en una carpeta separada, que no forma parte del producto.
   **Se decide en M11** (decisión diferida).
6. **Bindings PyO3 (#31)**: cancelado.
7. Generación de tipos desde el IDL (#19): sólo Rust y TypeScript.

## Criterio de lenguajes del usuario

- **Nada con indentación significativa** (Python, YAML como lenguaje de lógica, etc.).
  Rust y TypeScript usan llaves; la configuración en TOML/YAML se mantiene declarativa y
  plana.
- **Preferencia por lenguajes poco tipados para lo que escriba el usuario.** Se separan dos
  capas:
  - **Núcleo** (motor, ejecución, datos, estadística): tipado fuerte, porque los tipos son
    los que hacen cumplir los pilares (`Decimal` en vez de float, `PastView`/`FutureWindow`
    contra el sesgo de futuro, comandos firmados). Lo escriben agentes; el usuario no
    necesita tocarlo.
  - **Scripts del usuario** (indicadores propios, alertas personalizadas, consultas):
    lenguaje dinámico y con llaves, embebido y en sandbox (candidato: Rhai; alternativa:
    Lua). Se decide en el DSL de indicadores (#136).

## Alternativas descartadas

- **Mantener Python para research**: ecosistema más rico, pero agrega un segundo lenguaje
  de cálculo, bindings y conversiones. El usuario no lo quiere y lo que necesita M9 es
  implementable en Rust con un costo acotado.
- **Research en TypeScript**: peor rendimiento numérico y ecosistema estadístico más pobre
  que Rust.
- **Notebooks en Rust (evcxr)**: inmaduros; DuckDB SQL cubre la exploración.

## Consecuencias

- Algunos algoritmos estadísticos se implementan a mano. Por eso cada uno exige tests
  contra valores de referencia publicados o simulación.
- Si M12 necesita un modelo cuyo único soporte serio esté en Python, se escribe una ADR
  nueva con el caso concreto.
- El verificador de enlaces de la documentación pasa a ser un binario Rust
  (`bender-devtools`).
