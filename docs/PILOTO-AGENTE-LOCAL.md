# Piloto: delegar issues a un agente con modelo local

Objetivo: comprobar si un agente con un modelo local puede implementar issues usando
**sólo la documentación del repositorio**, sin contexto de estas conversaciones.

## Reglas del piloto

- El agente recibe únicamente: el repo clonado y el número de issue. Instrucción sugerida:
  > Leé `docs/AGENTES.md` y el issue #N (texto pegado o vía `gh issue view N`). Implementalo
  > en una rama `issue-N-<slug>` desde `main`, corré `make check` y prepará el PR según
  > `docs/AGENTES.md`.
- Nadie le explica nada que no esté en el repo. Cada vez que haga falta explicarle algo,
  eso es un **hueco de documentación**: se registra y se corrige en el repo, no en el chat.
- El PR lo revisa Claude (o vos) contra los criterios de aceptación antes de mergear.

## Issues candidatas, en orden

| Orden | Issue | Por qué sirve para el piloto | Requiere |
|---|---|---|---|
| 1 | #20 Tests de arquitectura y lints | chica, criterios mecánicos, toca Rust/TS/CI | #2 |
| 2 | #17 Esquema IDL: identidad, instrumentos, eventos | traducir documentación a código, sin lógica | #2 |
| 3 | #18 Esquema IDL: trading | igual que #17, comprueba consistencia | #17 |
| 4 | #23 Orden total y fusión de fuentes | algoritmo puro, bien especificado, con tests de propiedades | #19 |

#19 (generación de tipos entre lenguajes) **no** es buena candidata para el piloto: tiene
decisiones de herramientas que conviene que haga Claude.

## Cómo se evalúa cada intento

| Criterio | Cómo se mide |
|---|---|
| Criterios de aceptación | cuántos se cumplen, verificados uno por uno |
| CI | `make check` y la CI del PR en verde, sin desactivar nada |
| Pilares y alcance | no viola reglas de `AGENTES.md` ni adelanta otras issues |
| Autonomía | cuántas intervenciones humanas hicieron falta y de qué tipo |
| Huecos de documentación | lista; cada uno se corrige en el repo |
| Calidad | revisión de código: claridad, tests significativos, nada inventado |

Resultado por issue: **aceptada**, **aceptada con correcciones** o **rechazada** (se
rehace). Los resultados se anotan en la tabla de abajo.

## Registro

| Issue | Modelo | Resultado | Intervenciones | Huecos de documentación encontrados |
|---|---|---|---|---|
| | | | | |
