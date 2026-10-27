# Pilar 4 — Probabilidades calibradas y auditables

> Si BenderStyle dice "históricamente 67% de alcanzar el POC antes de invalidar el
> rechazo", tiene que poder mostrar **los casos usados, el período, el tamaño de muestra y
> cómo se validó** esa estimación. Si no puede, no lo dice.

---

## 1. Una estimación es un objeto, no un número

```
Estimate
  id
  pregunta:
    evento           rechazo en VAL de rango (definición v3)
    objetivo         tocar POC del mismo rango
    invalidación     cierre 4h por debajo de VAL − 0.5·ATR
    horizonte        96 h
  condicionamiento   { temporalidad: alta, toque: primero, vol_relativo: bajo }
  mercado            cripto (modelo separado de tradfi)
  muestra:
    casos            [case_id, …]            ← cada uno navegable
    período          2017-08 → 2026-10
    N                214   (N efectivo 171, ver §4)
    éxitos           143
  estimación:
    frecuencia cruda 66.8%
    ajustada         64.9% (encogida hacia la tasa base, ver §5)
    intervalo 90%    [59%, 70%]
    tasa base        51% (todos los rechazos en VAL, mismo horizonte)
  validación:
    método           walk-forward, purga 96 h, embargo 96 h
    calibración      tabla de confiabilidad, Brier, ECE (fuera de muestra)
    estabilidad      por época: 2017–19 / 2020–22 / 2023–26
  manifiesto         hash de datos + código + config (Pilar 2)
```

---

## 2. Auditabilidad en la UI

Click en "67%" → panel con:
- la pregunta exacta y las definiciones (con versión);
- la lista de casos: fecha, instrumento, nivel, resultado, tiempo hasta el resultado.
  **Cada caso se abre en la máquina del tiempo** en el momento del evento;
- la distribución de resultados (cuánto tardó, cuánto fue en contra antes de llegar);
- tasa base, intervalo, N efectivo;
- calibración fuera de muestra y estabilidad por época;
- el manifiesto, con un botón "recalcular" que tiene que dar el mismo número.

---

## 3. Las preguntas se registran antes de mirar

El mayor riesgo no es el modelo: es **buscar hasta encontrar**. Si probás 1.000
combinaciones de filtros, alguna da 67% por pura suerte.

- **Registro de hipótesis**: las preguntas y condicionamientos que el sistema puede
  mostrar se definen y versionan **antes** de evaluarlos.
- Lo que surge explorando se marca como **exploratorio** y no se muestra en alertas hasta
  validarse en un período posterior que no se usó para descubrirlo.
- Se lleva la cuenta de cuántas variantes se probaron por pregunta.

---

## 4. Casos que no son independientes

Diez toques del mismo nivel en dos días no son diez casos. Se calcula un **N efectivo**
(agrupando casos que comparten nivel y ventana de horizonte) y los intervalos se estiman
con bootstrap por grupos. Se muestran N crudo y N efectivo.

---

## 5. Muestras chicas

- **Encogimiento bayesiano** hacia la tasa base del grupo padre (ej. "todos los rechazos
  en VAL"): con N chico la estimación se acerca a la tasa base; con N grande manda la
  muestra.
- **N mínimo** para mostrar un número (configurable). Por debajo: "muestra insuficiente"
  y punto.
- Se muestran siempre la frecuencia cruda **y** la ajustada.

---

## 6. Estimaciones por modelo (ML)

Cuando la probabilidad sale de un modelo (gradient boosting, etc.) en lugar de una tabla:
- calibración con datos fuera de muestra (isotónica o Platt);
- se muestra la **confiabilidad en el tramo de la predicción actual**: "cuando el modelo
  dijo 60–70% fuera de muestra, ocurrió 64% de las veces (N=380)";
- como explicación, los **casos históricos más parecidos** y qué features pesaron más;
- comparación de skill contra la tasa base (Brier skill score). Si el modelo no le gana a
  la tasa base, se muestra la tasa base.

---

## 7. Calibración en vivo

Cada estimación emitida en vivo se registra y, cuando se cumple el horizonte, se resuelve.
Un tablero muestra la **calibración en vivo** (la prueba más honesta: datos que no
existían cuando se entrenó nada). Si se degrada, se avisa y la pregunta pasa a
"en revisión".

---

## 8. Lenguaje

El narrador sólo usa fórmulas como "históricamente, en N casos comparables, X%…". Nunca
"va a ir", "conviene", "hay que". Y si el número no supera el N mínimo o no tiene
validación, no aparece.
