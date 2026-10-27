# ADR-0013 — Servidor MCP de análisis abierto a Claude, Codex y el LLM local

- Estado: **aceptada**. Niveles de datos confirmados por el usuario como configuración inicial: `mercado` para clientes en la nube (Claude, Codex), `completo` sólo para el LLM local
- Fecha: 2026-10-27
- Amplía: [ADR-0006](ADR-0006-aislamiento-llm.md) (herramientas y permisos de escritura)
- Diseño: [09-mcp-de-analisis.md](../arquitectura/09-mcp-de-analisis.md)

## Contexto

El usuario quiere que asistentes externos (Claude, Codex) puedan analizar su análisis: ver
el mercado en cualquier horizonte temporal, leer sus dibujos y notas, revisar sus trades y
escribir. El plan tenía un `llm-gateway` sólo para el LLM local, con escritura limitada a
notas del diario.

## Decisión

1. **`bender-mcp`**: un único servidor MCP, con catálogo cerrado, que sirve al LLM local y a
   clientes externos. Reemplaza al `llm-gateway` como servidor de herramientas.
2. **Sesiones ancladas a un instante** (`as_of` obligatorio para toda la sesión): la IA no
   puede ver el futuro de T. Lo garantiza el núcleo temporal.
3. **Escritura ampliada, pero sólo en la capa de IA**: notas, dibujos, niveles, borradores
   de hipótesis y alertas **propuestas**. Todo append-only, atribuido y versionado. Tus
   registros nunca se modifican; aceptar una propuesta la copia a tu capa.
4. **Política de datos por cliente** (`mercado` / `trades-relativos` / `completo`), aplicada
   por el servidor. Default para clientes en la nube: `mercado`.
5. **Sin exposición a internet**: stdio local o HTTP por Tailscale.
6. **Entrega incremental**: el servidor base (mercado + tu análisis + capa de IA + sesiones
   ancladas) entra **después de P0**. Cada milestone posterior agrega sus herramientas en
   su propio issue.

Se mantienen sin cambios: ninguna herramienta de órdenes, ningún camino a `execution` y las
pruebas de aislamiento.

## Alternativas descartadas

- **Un servidor distinto para cada modelo**: dos catálogos que divergen.
- **Que la IA escriba en la capa del usuario**: mezcla análisis propio con propuestas y
  rompe la atribución.
- **Que la IA cree alertas activas**: una alerta no ejecuta, pero condiciona tus
  decisiones; entra como propuesta.
- **Exponer el MCP públicamente** (para conectores web): superficie de ataque y datos
  personales en internet; se reconsidera con una ADR propia si hace falta.
- **Mandar todo a la nube por defecto**: el capital y el PnL en dinero son datos sensibles;
  el usuario elige el nivel.

## Consecuencias

- `docs/arquitectura/08-aislamiento-llm.md` aplica al servidor MCP completo, con el permiso
  extra `write:ai_layer`.
- La UI necesita una capa de IA con aceptar y descartar.
- Se puede evaluar a la IA con honestidad: un análisis hecho en una sesión anclada a T se
  compara con lo que pasó después, igual que una estimación del motor.
