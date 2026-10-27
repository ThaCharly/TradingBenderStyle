# Pilar 4 — Probabilidades calibradas y auditables

> Si BenderStyle dice "históricamente 67% de alcanzar el POC antes de invalidar el
> rechazo", tiene que poder mostrar **los casos usados, el período, el tamaño de muestra,
> cómo se validó y contra qué se comparó**. Si no puede, no lo dice.
> Decisión: [ADR-0003](../decisiones/ADR-0003-calibracion-y-validacion.md).

---

## 1. La pregunta tiene que estar bien definida

Toda estimación responde una pregunta con tres resultados posibles, definida de antemano:

```
evento         rechazo en VAL de canal 4H (definición v3)
objetivo       tocar POC del mismo canal                    → resultado TARGET
invalidación   cierre 4H por debajo de VAL − 0.5·ATR(14,4H) → resultado STOP
horizonte      96 h; si no pasó ninguna de las dos          → resultado TIMEOUT
```

La probabilidad mostrada es `P(TARGET)`. Las de `STOP` y `TIMEOUT` también se calculan y
están en el panel de auditoría. Si dentro de una misma vela se tocan objetivo e
invalidación, se resuelve con datos de menor temporalidad (hasta trades); si no hay datos
para resolverlo, el caso se marca **ambiguo** y se excluye y se cuenta.

---

## 2. Calibración no es lo mismo que capacidad predictiva

Son dos propiedades distintas, se miden por separado y **se exigen las dos**:

- **Calibración**: cuando el sistema dice 65%, ¿pasa el 65% de las veces? Un modelo que
  siempre dice la tasa base (ej. 51%) está **perfectamente calibrado y no sirve para
  nada**.
- **Capacidad predictiva** (discriminación, resolución): ¿el sistema distingue los casos
  que van a funcionar de los que no? Un modelo puede ordenar bien los casos (alto AUC) y
  estar descalibrado (dice 90% cuando pasa 60%).

| Mide | Métrica | Lectura |
|---|---|---|
| Calibración | diagrama de confiabilidad (bins por cuantiles) | puntos sobre la diagonal |
| Calibración | **pendiente e intercepto de calibración** (regresión logística del resultado sobre el logit predicho) | pendiente ≈ 1 (< 1 = sobreconfiado), intercepto ≈ 0 |
| Calibración | ECE con bins adaptativos | informativo; sensible a cómo se arman los bins |
| Calibración | término de **confiabilidad** de la descomposición de Murphy del Brier | cuanto menor, mejor |
| Discriminación | ROC AUC, PR AUC | > 0.5; comparar contra los modelos base |
| Discriminación | término de **resolución** de la descomposición de Murphy | cuanto mayor, mejor |
| Ambas | Brier score, log loss (reglas de puntuación propias) | comparar contra los modelos base |
| Ambas | **Brier skill score** contra un modelo base: `BSS = 1 − Brier_modelo / Brier_base` | > 0 = le gana al modelo base |

---

## 3. Modelos base (los que hay que superar)

Ninguna estimación se muestra como "con información" si no le gana a estos:

| ID | Modelo base | Qué controla |
|---|---|---|
| **B0** | Tasa base incondicional de la pregunta (todos los eventos de ese tipo) | ¿el condicionamiento agrega algo? |
| **B1** | Tasa base por temporalidad y por régimen de volatilidad (terciles de ATR relativo) | ¿agrega algo más allá del contexto obvio? |
| **B2** | **Niveles placebo**: el mismo evento definido sobre niveles al azar, a la misma distancia del precio y con la misma geometría de objetivo e invalidación | **¿el nivel (VAL, POC…) importa, o cualquier línea daría lo mismo?** |
| **B3** | **Geometría pura**: probabilidad de tocar el objetivo antes que la invalidación en un paseo aleatorio sin deriva, `P ≈ d_stop / (d_target + d_stop)`, ajustada por el horizonte con la volatilidad actual | **¿el 67% sale sólo de que el objetivo estaba más cerca que el stop?** |
| **B4** | Regresión logística con 3–5 features simples (distancias en ATR, volumen relativo, temporalidad) | ¿el modelo complejo justifica su complejidad? |

B2 y B3 son los más importantes. Muchos porcentajes "impresionantes" son geometría:
si el objetivo está a 1 ATR y la invalidación a 2 ATR, un paseo aleatorio ya da ~67%.
**Si no le gana a B3, el narrador lo dice**: "el nivel no agrega información más allá de
la geometría del movimiento".

---

## 4. Cómo se calibra

- **Estimaciones por frecuencia** (tablas condicionadas): modelo **beta-binomial
  jerárquico**. El prior de cada celda es la tasa del grupo padre (empirical Bayes), así
  con N chico la estimación se acerca a la tasa padre. Se reporta la media posterior y el
  intervalo de credibilidad del 90%. No se recalibra después: el encogimiento ya es la
  calibración.
- **Estimaciones por modelo** (gradient boosting y similares):
  - calibrador ajustado en un **pliegue de calibración propio**, posterior al de
    entrenamiento y anterior al de prueba, **nunca** sobre el de prueba;
  - **Platt** (logístico) por defecto; **isotónico** sólo con más de ~1.000 casos en el
    pliegue de calibración; para 3 clases, *temperature/vector scaling*;
  - se elige el calibrador por log loss en el pliegue de calibración, no en prueba.

---

## 5. Cómo se valida fuera de muestra

### 5.1 Walk-forward con purga y embargo

```
|── entrenamiento ──|purga|── calibración ──|purga|── prueba ──|embargo|  → se corre la ventana
```

- Ventana expansiva, pasos de 6 meses (configurable).
- **Purga**: se eliminan del entrenamiento y de la calibración los casos cuyo horizonte de
  etiqueta se solapa con el período siguiente.
- **Embargo**: después de cada período de prueba, un margen igual al horizonte antes de
  volver a usar datos para entrenar.
- Todas las métricas de §2 se reportan **sobre la concatenación de los períodos de prueba**,
  y también **por época** (2017–19, 2020–22, 2023–26) para ver estabilidad.

### 5.2 Caja fuerte

Los **últimos 12 meses** quedan sellados. Cada versión de modelo se evalúa contra la caja
fuerte **una sola vez**, al final, y el resultado se registra. Si se toca la caja fuerte
para iterar, deja de ser caja fuerte (se registra y se sella un período nuevo).

### 5.3 Fuera de activo

Un modelo entrenado en BTC se evalúa también en ETH (y el de tradfi entre NQ y ES). No es
requisito superarlo, pero se reporta: si sólo funciona en un activo, probablemente esté
sobreajustado.

### 5.4 En vivo

Cada estimación emitida en vivo se registra y se resuelve al cumplirse el horizonte. El
tablero de **calibración en vivo** usa las mismas métricas. Es la prueba más honesta.

### 5.5 Incertidumbre de las métricas

Los casos no son independientes (toques repetidos del mismo nivel, horizontes solapados):
- **N efectivo** agrupando casos que comparten nivel y ventana;
- intervalos de las métricas por **bootstrap por bloques** (bloques temporales de longitud
  igual al horizonte);
- comparación modelo vs base con la diferencia de pérdidas por caso y bootstrap por
  bloques del promedio de esa diferencia.

---

## 6. Criterios de promoción (para que una estimación se muestre)

Un modelo o una tabla pasa a **"publicable"** sólo si, en walk-forward **y** en caja
fuerte:

1. N efectivo ≥ 30 en la celda o el tramo mostrado.
2. Pendiente de calibración en [0.8, 1.2] e intercepto en [−0.1, 0.1] (logit).
3. BSS contra **B3 y B2** > 0 con el intervalo del 90% por bootstrap **excluyendo el 0**.
4. Sin degradación fuerte entre épocas (BSS de ninguna época < −0.05).

Los umbrales son defaults iniciales, versionados en la configuración. Si no se cumplen,
se muestra el mejor modelo base y se dice por qué.

---

## 7. Registro de hipótesis

- Las preguntas y los condicionamientos que se pueden publicar se registran **antes** de
  evaluarlos (archivo versionado).
- Todo lo que surge explorando es **exploratorio**: no se muestra en alertas hasta pasar
  §6 en un período que no se usó para descubrirlo.
- Se cuenta cuántas variantes se probaron por pregunta; ese número aparece en la
  auditoría.

---

## 8. El objeto `Estimate` y la auditoría

```
Estimate
  pregunta (evento, objetivo, invalidación, horizonte, versiones de definiciones)
  condicionamiento, mercado
  método: tabla_beta_binomial | modelo(id, versión, calibrador)
  valor: P(TARGET), P(STOP), P(TIMEOUT), intervalo 90%
  muestra: case_ids, período, N, N efectivo, ambiguos excluidos
  bases: valor y BSS contra B0…B4 (con intervalos)
  validación: walk-forward (métricas §2 por época), caja fuerte, fuera de activo
  calibración en vivo hasta la fecha
  estado: publicable | exploratorio | en revisión
  manifiesto (Pilar 2)
```

Al hacer click en "67%" en la UI se abre:
- la pregunta exacta y sus definiciones;
- la lista de casos, cada uno navegable en la máquina del tiempo;
- la distribución de resultados (tiempo hasta el resultado, excursión adversa);
- la comparación contra B0–B4;
- el diagrama de confiabilidad, la pendiente, la resolución y el BSS por época;
- un botón "recalcular" que tiene que dar exactamente el mismo número.

---

## 9. Lenguaje

El narrador usa fórmulas como "históricamente, en N casos comparables, X%… (vs Y% por
geometría)". Nunca "va a ir", "conviene", "hay que". Si la estimación no es
publicable, no aparece el porcentaje.
