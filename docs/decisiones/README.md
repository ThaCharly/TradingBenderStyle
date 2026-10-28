# Registro de decisiones (ADR)

| ADR | Decisión | Estado |
|---|---|---|
| [0001](ADR-0001-motor-grafico.md) | Lightweight Charts v5 detrás de una capa de escena propia; renderer propio sólo si falla el spike | aceptada (revoca plan v2) |
| [0002](ADR-0002-garantias-ejecucion-futuros.md) | Futuros sin asumir brackets atómicos: invariantes, sagas, reconciliación | aceptada (corrige plan v2) |
| [0003](ADR-0003-calibracion-y-validacion.md) | Calibración ≠ discriminación; walk-forward, caja fuerte, modelos base B0–B4 | aceptada |
| [0004](ADR-0004-nucleo-temporal-primero.md) | Núcleo temporal antes que cualquier consumidor; un solo Runner | aceptada |
| [0005](ADR-0005-topologia-distribuida.md) | Componentes desplegables en servidor, PC y Android | aceptada |
| [0006](ADR-0006-aislamiento-llm.md) | LLM sólo consulta y escribe notas propias; sin camino a ejecución | aceptada |
| [0007](ADR-0007-almacenamiento-minimo.md) | Parquet + DuckDB + Postgres; ClickHouse diferido con criterio medible | aceptada (simplifica plan v2) |
| 0008 | Shell móvil y canal de push | reservada para el spike #45 |
| [0009](ADR-0009-presupuesto-de-disco.md) | Presupuesto de disco ~1 TB y retención (crudo 90 días, L2 acotado) | retención reemplazada por 0014 |
| [0010](ADR-0010-archivado-y-paso-a-produccion-del-grabador.md) | Archivado comprimido de irreemplazables antes de borrar; gate para mover el grabador a la mini-PC | aceptada |
| [0011](ADR-0011-completitud-de-datos-y-recuperacion-de-huecos.md) | Completitud por intervalo (completo / recuperado / incompleto); recuperar antes de declarar incompleto, sin ocultar huecos | aceptada |
| [0012](ADR-0012-sin-python.md) | Sin Python: research, estadística y herramientas en Rust; LoRA se decide en M11 | aceptada |
| [0013](ADR-0013-mcp-de-analisis.md) | Servidor MCP de análisis para Claude, Codex y el LLM local; sesiones ancladas a un instante; escritura sólo en la capa de IA | aceptada |
| [0014](ADR-0014-perfiles-de-grabacion-y-resolucion-base.md) | Perfiles de grabación (liviano / al tick), base de 1m con resúmenes por banda de precio, historia descargada por el usuario de la fuente oficial, términos de uso por dato | aceptada |
| [0015](ADR-0015-servidor-portable.md) | Servidor portable: hoy VM de GCP en Santiago, migrable a la mini-PC; sin servicios administrados del proveedor | aceptada |

Formato: contexto → decisión → alternativas descartadas → consecuencias. Una ADR aceptada
no se edita para cambiar la decisión: se escribe una nueva que la reemplaza.
