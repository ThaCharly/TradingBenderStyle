# Revisión arquitectónica crítica del plan v2

- Fecha: 2026-10-27
- Alcance: `docs/PLAN.md` v2 y `docs/arquitectura/01–04`
- Resultado: plan v3 (este commit), ADR-0001 a ADR-0007, documentos 00 y 05–08

Cada hallazgo indica severidad (**alta**: podía causar pérdida de plata o invalidar
resultados; **media**: costo o retrabajo significativo; **baja**: claridad), qué decía
el plan y qué se decidió.

---

## Puntos pedidos

### H1 — Motor gráfico propio sin fundamento técnico · media

- **Plan v2**: renderer propio "porque con tiempo infinito, controlar el renderer
  permite…". No comparaba capacidades.
- **Problema**: el tiempo ilimitado no paga el mantenimiento. Lo que da valor (escena de
  dominio, interacción, overlays del motor) se construye igual sobre cualquier renderer.
  Un renderer propio duplica lo que Lightweight Charts ya resuelve (ejes, HiDPI, gestos,
  inercia).
- **Decisión**: [ADR-0001](../decisiones/ADR-0001-motor-grafico.md). Lightweight Charts
  v5 detrás de una capa de escena propia (`ChartScene`). Criterios de salida medibles y un
  spike que los mide en PC y en el S25. KLineChart como alternativa de respaldo.

### H2 — Brackets atómicos supuestos · **alta**

- **Plan v2**: "bracket atómico si el exchange lo soporta; si no, en secuencia con
  verificación y rollback".
- **Problemas**:
  1. Los lotes no son atómicos.
  2. Un timeout no dice si la orden entró.
  3. "Rollback" cancelando la entrada no deshace fills ya ocurridos.
  4. Los fills parciales cambian lo que hay que proteger.
  5. No había estados para "expuesto", "incierto" ni "modificado desde afuera".
- **Decisión**: [ADR-0002](../decisiones/ADR-0002-garantias-ejecucion-futuros.md) y
  [doc 06](../arquitectura/06-ejecucion-futuros.md). Estrategias P0/P1/P2 según
  capacidades **verificadas**, sagas con estados explícitos, invariantes INV-1…7 con
  monitor continuo, escalera de exposición, reconciliación, *make-before-break*,
  simulador con inyección de fallas y tests de caos.

### H3 — Calibración confundida con capacidad predictiva · **alta**

- **Plan v2**: reliability, Brier y ECE juntos; comparación sólo contra la tasa base.
- **Problemas**: no separaba calibración de discriminación; no había modelo base de
  geometría (un 67% puede ser sólo que el objetivo estaba más cerca) ni de niveles
  placebo; calibraba sin especificar sobre qué datos; no había criterio de publicación.
- **Decisión**: [ADR-0003](../decisiones/ADR-0003-calibracion-y-validacion.md) y
  [Pilar 4 reescrito](../arquitectura/04-probabilidades-auditables.md).

### H4 — Motor reproducible demasiado tarde · **alta**

- **Plan v2**: Fase 2, después del grabador; el grabador y la gráfica podían nacer con
  reglas temporales propias.
- **Decisión**: [ADR-0004](../decisiones/ADR-0004-nucleo-temporal-primero.md) y
  [doc 05](../arquitectura/05-nucleo-temporal.md). El núcleo temporal es M1, con
  especificación normativa. El grabador usa su formato desde el primer byte y hay un solo
  `Runner` para vivo, replay, backtest y dataset. Los tipos `PastView`/`FutureWindow`
  impiden que una feature mire el futuro.

### H5 — Topología implícita en una sola máquina · media

- **Plan v2**: todo el backend en la mini-PC. No decía dónde está la GPU, ni cómo se
  comunican los componentes, ni qué pasa sin conexión.
- **Decisión**: [ADR-0005](../decisiones/ADR-0005-topologia-distribuida.md) y
  [doc 07](../arquitectura/07-topologia-distribuida.md). Interfaces con transporte en
  proceso o en red, `execution` singleton sólo en el servidor, workers sin estado,
  sincronización de datos por hash, perfiles de nodo y comandos nunca encolados offline.

### H6 — Aislamiento del LLM sólo enunciado · **alta**

- **Plan v2**: "el registro de herramientas es de sólo lectura por construcción". No
  contemplaba que en Android el LLM convive **en la misma app** que opera.
- **Decisión**: [ADR-0006](../decisiones/ADR-0006-aislamiento-llm.md) y
  [doc 08](../arquitectura/08-aislamiento-llm.md). Cinco capas (código, herramientas,
  credenciales con biometría por operación, red, presentación/proceso aislado) y seis
  pruebas obligatorias en CI.

---

## Hallazgos adicionales

| # | Hallazgo | Sev. | Resolución |
|---|---|---|---|
| H7 | ClickHouse en una mini-PC sin necesidad medida | media | [ADR-0007](../decisiones/ADR-0007-almacenamiento-minimo.md): Parquet + DuckDB + Postgres; ClickHouse con criterio medible |
| H8 | No se sabe si la GPU está en la PC o en la mini-PC | media | Perfiles de nodo: `worker` y `llm-gateway` se mueven por configuración. Pregunta abierta |
| H9 | Los latidos de reloj no se grababan: las condiciones por tiempo no eran reproducibles | **alta** | Doc 05 §3: latidos grabados |
| H10 | No había regla para datos tardíos ni para revisiones de velas | media | Doc 05 §5: marca de agua, `BarRevision` |
| H11 | Velas compuestas multi-venue sin regla de cierre | media | Doc 05 §6: espera máxima `M` y fuentes incompletas registradas |
| H12 | Ambigüedad intra-vela (objetivo e invalidación en la misma vela) | media | Pilar 4 §1: resolución con menor temporalidad o caso ambiguo excluido y contado |
| H13 | El LLM podía "sugerir" órdenes en la UI | **alta** | ADR-0006: descartado |
| H14 | Push en Android sin definir (las restricciones de segundo plano impiden conexiones largas) | media | Spike en M3 |
| H15 | Tauri 2 en Android es relativamente nuevo; riesgo para WebView y plugins nativos | media | Spike en M3; si falla, cliente Android nativo con la misma escena en WebView |
| H16 | Grabar L2 de todos los venues puede llenar el disco | media | Grabador con top-N configurable por venue, monitoreo de disco y retención |
| H17 | Indicadores calculados en el cliente divergirían de las alertas | media | Indicadores en `bender-engine`, servidos por `query` (una sola implementación) |
| H18 | Un solo proceso `execution` no estaba garantizado (dos instancias = órdenes dobles) | **alta** | INV-5: lease en Postgres |

---

## Qué no cambió

Los cuatro pilares, la prioridad de exchanges (Binance → BloFin → BingX → MEXC/Bybit), el
servidor en casa y el LLM 4B en el teléfono se mantienen.
