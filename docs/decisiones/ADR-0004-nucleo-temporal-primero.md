# ADR-0004 — Núcleo temporal antes que cualquier consumidor

- Estado: **aceptada**
- Fecha: 2026-10-27
- Especificación: [05-nucleo-temporal.md](../arquitectura/05-nucleo-temporal.md)

## Contexto

El plan v2 ponía el motor reproducible en la Fase 2, después del grabador. Pero el
grabador ya **decide reglas temporales**: qué marcas de tiempo guarda, cómo ordena, cómo
marca huecos, si graba latidos. Y si la gráfica o las alertas se construyen antes del
núcleo, nacen con lógica temporal propia (cuándo cierra una vela, qué es "ahora") que
después diverge del backtest.

## Decisión

1. **M1 = núcleo temporal**: sobre de eventos, orden total, reloj por eventos, latidos,
   marca de agua y tardíos, velas, `as_of`, checkpoints, manifiestos y los cinco tests
   normativos. Como librería pura, sin red.
2. El grabador (M2) **usa el formato del núcleo desde el primer byte**: graba sobres
   completos, latidos y huecos.
3. **Un solo `Runner`** con cuatro modos (`live`, `replay`, `backtest`, `dataset`). Las
   alertas, el motor, el simulador de paper y los datasets de entrenamiento son
   consumidores del `Runner`; ninguno tiene reloj propio.
4. La gráfica pide velas a `query`, que las sirve desde el núcleo (`as_of(ahora)` en
   vivo, `as_of(T)` en la máquina del tiempo). Nunca agrega trades por su cuenta.

## Alternativas descartadas

- **Construir gráfica y alertas primero y "unificar después"**: cada consumidor nuevo
  agrega una implementación más a reconciliar.
- **Motor reproducible sólo para backtest** (y otro código para vivo): es exactamente la
  divergencia que el Pilar 2 prohíbe.
- **Usar el reloj del sistema y "tener cuidado"**: no es verificable.

## Consecuencias

- La primera UI visible llega un milestone más tarde. Se mitiga con el spike de gráfica
  (M3) en paralelo con datos estáticos.
- El grabador puede arrancar **en cuanto existan el sobre y el formato de almacenamiento**
  (primeros issues de M1), sin esperar al resto del núcleo: se gana historia grabada
  cuanto antes.
