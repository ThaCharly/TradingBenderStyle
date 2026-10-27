# ADR-0002 — Garantías de ejecución de futuros sin asumir brackets atómicos

- Estado: **aceptada** (corrige "bracket atómico si el exchange lo soporta" del plan v2)
- Fecha: 2026-10-27
- Diseño detallado: [06-ejecucion-futuros.md](../arquitectura/06-ejecucion-futuros.md)

## Contexto

El plan v2 decía "entrada + SL + TPs como bracket atómico; si no, en secuencia con
rollback". Eso supone que existe atomicidad en algún exchange y que el rollback es
confiable. Ninguna de las dos cosas está garantizada: los lotes se procesan orden por
orden, las respuestas pueden perderse (timeout con la orden aceptada) y los fills
parciales llegan en cualquier momento.

## Decisión

1. **Ningún exchange se trata como atómico.** La seguridad se basa en **invariantes**
   (INV-1…INV-7) vigilados de forma continua y con tiempos acotados.
2. **Protección siempre nativa** en el exchange, con estrategia elegida por capacidad
   **verificada**: P0 pre-armada > P1 adjunta > P2 post-fill.
3. **Saga explícita** por trade, con estados de exposición (`EXPOSED`), incertidumbre
   (`UNCERTAIN`) y modificación externa (`EXTERNALLY_MODIFIED`) de primera clase.
4. **Escalera de exposición** con cierre de emergencia por defecto al vencer `T_exp`.
5. **Resultados desconocidos** (`UNKNOWN`) → consulta por `client_order_id` determinista;
   nunca reenvío a ciegas.
6. **Reconciliación** periódica y por eventos contra el exchange, que es la fuente de
   verdad de los hechos (fills, posiciones). Nosotros somos la fuente de verdad de la
   intención.
7. *Make-before-break* para todo cambio de protección.
8. Capacidad reservada en el limitador de tasa para protección y cancelaciones.

## Alternativas descartadas

- **Confiar en el lote como atómico**: no lo es.
- **Rollback cancelando la entrada si falla el SL**: si la entrada ya tuvo fills, cancelar
  no deshace la posición; la acción correcta es proteger o cerrar.
- **Stops locales en el servidor**: con un corte de internet en casa, la posición queda
  desnuda.
- **Cancel-on-disconnect del exchange**: cancelaría también los stops.
- **Stop-limit como SL por defecto**: en un gap puede no llenarse; se prefiere
  stop-market.

## Consecuencias

- Más estados y más tests, pero cada uno corresponde a una situación que **va a pasar**.
- Antes de habilitar un exchange para futuros reales hay que verificar su tabla de
  capacidades en testnet/demo y pasar la suite de fallas del simulador.
- La UI muestra la ventana de exposición real de cada trade.
