# Traspaso: de la planificación a la ejecución

La planificación está cerrada. **Toda la ejecución la hace una sesión de Claude**, distinta
de la de planificación. Todo lo necesario está en el repo; nada depende de conversaciones
anteriores.

## 1. Estado al cierre de la planificación

| Qué | Estado |
|---|---|
| Plan, arquitectura y decisiones (ADR-0001…0013) | cerrados; ver [PLAN](PLAN.md) y [ADRs](decisiones/README.md) |
| Issues | 16 milestones (M0–M15: épicos #1, #3–#16, #143) + seguimiento del prototipo #141 |
| #2 entorno base y CI | hecho y mergeado en `main` |
| `main` | rama por defecto |
| Código funcional | ninguno todavía |

## 2. Roles

- **Sesión de planificación**: decisiones de arquitectura, ADRs, issues y cambios de
  alcance. Si en la ejecución aparece algo que cambia el plan, se registra como ADR nueva
  y se avisa al usuario.
- **Sesión de ejecución**: implementa los issues de a uno, con PR a `main`.
- **Usuario**: decide lo que condiciona el proyecto y corre en su PC lo que necesita
  hardware real.

## 3. Orden de trabajo para P0 ([PROTOTIPO.md](PROTOTIPO.md), seguimiento #141)

Por dependencias, siempre tomando el primero desbloqueado:

1. **Cimientos**: #17 → #18 → #19, y #20 en paralelo; después #21.
2. **Núcleo temporal**: #22, #23 → #24 → #25 → #26, #27 → #28 → #29 → #30.
3. **Grabador**: #32, después #156.
4. **Gráfica**: #44 (spike) en cualquier momento después de #2, sobre todo mientras otra
   cosa esté bloqueada; después #46 → #47 → #48 → #49 → #54.
5. **Integración**: #155.

Después de P0: P1 (grabador en producción) y la base del MCP (#143), según el
[ROADMAP](ROADMAP.md).

**Cambio de almacenamiento ([ADR-0014](decisiones/ADR-0014-perfiles-de-grabacion-y-resolucion-base.md)):**
perfiles de grabación (liviano por defecto, al tick opcional), base de 1m con resúmenes por
banda de precio, historia descargada por el usuario de la fuente oficial y términos de uso
por dato. El grabador de P0 (#32) se termina según su issue y lo nuevo va en issues
aparte: #43 (perfiles y ventanas), #174, #175, #176, #177, más ajustes en #39–#41, #46,
#48, #50 y #139. El núcleo temporal ya mergeado no cambia: sigue soportando segundos.

## 4. Cómo se trabaja cada issue

Reglas completas en [AGENTES.md](AGENTES.md) y [RAMAS.md](RAMAS.md). En resumen:
- Verificar que los "Bloqueado por" estén cerrados.
- Rama desde `main` actualizado; un issue por PR. Si la plataforma asigna un nombre de rama
  propio, se usa ese.
- `make check` en verde; PR con `Closes #<n>`, criterios tildados con su verificación y
  "Huecos de documentación".
- **Merge**: la sesión de ejecución mergea con squash cuando la CI está en verde y todos los
  criterios están verificados. Si algo no se pudo cumplir, no mergea y lo explica.
- Marcar el issue en #141.
- Si el issue entrega una funcionalidad visible, actualizar su **Estado** en
  [FUNCIONALIDADES.md](FUNCIONALIDADES.md) en el mismo PR.

## 5. Lo que necesita la PC del usuario

La sesión de ejecución corre en un contenedor que se apaga, sin acceso a tu hardware. Para
estos casos deja todo listo, indica exactamente qué correr y espera los resultados:
- reporte de 24 h del grabador (#32) y validación de velas (#156);
- demo `make dev` (#155);
- mediciones del spike de gráfica en la PC y en el Samsung S25 (#44).

## 6. Decisiones

- Menores: las toma la sesión de ejecución y las documenta en el PR.
- Las que condicionan el proyecto van al usuario con una recomendación. Las próximas:
  resultado del spike #44 (confirma o reemplaza ADR-0001) y consumo real de disco (ajusta
  ADR-0009).
- Lista completa de decisiones diferidas y sus disparadores:
  [PLAN §13](PLAN.md#13-decisiones-cuáles-ahora-y-cuáles-después).
