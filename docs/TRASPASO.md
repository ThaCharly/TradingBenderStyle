# Traspaso: de la planificación a la ejecución

La planificación está cerrada. Este documento es el punto de entrada para la **sesión de
ejecución** (Claude) y para el **agente local**. Todo lo necesario está en el repo; nada
depende de conversaciones anteriores.

## 1. Estado al cierre de la planificación

| Qué | Estado |
|---|---|
| Plan, arquitectura y decisiones (ADR-0001…0013) | cerrados; ver [PLAN](PLAN.md) y [ADRs](decisiones/README.md) |
| Issues | 15 milestones (M0–M15, épicos #1, #3–#16, #143) + seguimiento del prototipo #141 |
| #2 entorno base y CI | hecho en el **PR #142**, CI verde, pendiente de merge |
| Código funcional | ninguno todavía |

## 2. Lo que tiene que hacer el usuario antes de arrancar

1. Mergear el **PR #142** con **squash**. Cierra #2 y trae toda la documentación a `main`.
2. Configurar `main` como rama por defecto y su protección (checks `docs`, `rust`, `ts`,
   `buf`): ver [RAMAS.md](RAMAS.md).
3. Opcional: crear Milestones reales de GitHub y pedirle a Claude que asigne cada issue
   según su etiqueta `M*`.

## 3. Orden de trabajo

El objetivo inmediato es el **prototipo P0** ([PROTOTIPO.md](PROTOTIPO.md), seguimiento #141).
Después: P1 (grabador en producción) y la base del MCP (#143). El resto sigue el
[ROADMAP](ROADMAP.md).

Reparto sugerido para P0:

| Quién | Issues | Por qué |
|---|---|---|
| Agente local (piloto) | #20 → #17 → #18 → #23 | chicos y bien especificados; miden si la documentación alcanza ([PILOTO-AGENTE-LOCAL.md](PILOTO-AGENTE-LOCAL.md)) |
| Claude (sesión de ejecución) | #19, #21, #22, #24–#30 | núcleo temporal: es el corazón y lo más delicado |
| Claude, en paralelo | #44 (spike de gráfica) | arranca apenas está mergeado #2 |
| Quien quede libre, cuando se desbloqueen | #32, #156, #46, #47, #48, #49, #54, #155 | grabador, validación, gráfica, integración |

Si el piloto con el agente local no rinde, Claude toma esos issues y el resultado se anota
en el registro del piloto.

## 4. Cómo arrancar la sesión de ejecución

Prompt sugerido para la nueva sesión de Claude:

> Sos el ejecutor de TradingBenderStyle. Leé `docs/TRASPASO.md`, `docs/AGENTES.md` y
> `docs/RAMAS.md`. Trabajá los issues de P0 (#141) en el orden de dependencias que asigna
> `docs/TRASPASO.md` §3, de a uno: rama `issue-<n>-<slug>` desde `main`, `make check`,
> PR con `Closes #<n>`. No adelantes funcionalidades fuera del issue. Las decisiones que
> condicionen el proyecto me las traés; las menores las tomás vos y las documentás.

Para el agente local: el número de issue y la instrucción de
[PILOTO-AGENTE-LOCAL.md](PILOTO-AGENTE-LOCAL.md).

## 5. Reglas que siguen vigentes

- Un issue por PR, squash a `main`, `make check` verde.
- No adelantar funcionalidades de fases posteriores.
- Si algo contradice la documentación: se para y se pregunta (o se documenta el hueco).
- Decisiones que condicionan el proyecto → al usuario. Decisiones menores → criterio
  técnico, documentado en el PR o en una ADR.
- Sin Python; nada con indentación significativa ([ADR-0012](decisiones/ADR-0012-sin-python.md)).

## 6. Decisiones diferidas

Están listadas con su disparador en [PLAN §13](PLAN.md#13-decisiones-cuáles-ahora-y-cuáles-después).
Las próximas que van a aparecer en P0:
- resultado del spike de gráfica (#44) → confirma o reemplaza ADR-0001;
- consumo real de disco (reporte de #32) → confirma o ajusta ADR-0009.
