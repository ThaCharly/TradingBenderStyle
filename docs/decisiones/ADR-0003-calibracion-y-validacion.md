# ADR-0003 — Calibración, validación fuera de muestra y modelos base

- Estado: **aceptada**
- Fecha: 2026-10-27
- Diseño detallado: [04-probabilidades-auditables.md](../arquitectura/04-probabilidades-auditables.md)

## Contexto

El plan v2 hablaba de "probabilidades calibradas" pero mezclaba calibración con capacidad
predictiva y comparaba sólo contra la tasa base. Una estimación puede estar calibrada y no
servir (siempre dice la tasa base), o discriminar bien y estar descalibrada. Y un
porcentaje alto puede venir sólo de la geometría (objetivo más cerca que la
invalidación), no del nivel.

## Decisión

1. Preguntas de **tres resultados** (objetivo, invalidación, timeout) con horizonte fijo.
2. **Calibración y discriminación se miden por separado** (pendiente e intercepto de
   calibración, confiabilidad vs resolución de Murphy, AUC) y **se exigen las dos**.
3. **Modelos base obligatorios** B0–B4, con B2 (niveles placebo) y B3 (geometría pura de
   paseo aleatorio) como vara principal.
4. **Calibración**: beta-binomial jerárquico para tablas; Platt/isotónico en un pliegue de
   calibración separado para modelos.
5. **Validación**: walk-forward con purga y embargo, caja fuerte de 12 meses de un solo
   uso, evaluación fuera de activo y calibración en vivo.
6. **Criterios de promoción** numéricos y versionados para que algo se publique.

## Alternativas descartadas

- **Validación cruzada k-fold aleatoria**: mezcla futuro con pasado en series temporales.
- **Calibrar sobre el set de prueba**: optimista por construcción.
- **ECE como única métrica**: depende de los bins y no mide discriminación.
- **Comparar sólo contra la tasa base**: no detecta que el resultado sea pura geometría ni
  que cualquier nivel daría lo mismo.
- **Modelos sin calibrar mostrados como probabilidad** (salida cruda de gradient boosting):
  suelen estar descalibrados.

## Consecuencias

- Muchas estimaciones no van a pasar los criterios. Es el resultado esperado: el sistema
  muestra el mejor modelo base y dice por qué.
- La implementación de B2 y B3 es parte del alcance mínimo de M9, no un extra.
