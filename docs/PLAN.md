# TradingBenderStyle — Plan maestro

> Un TradingView personal, sin ruido, con ejecución en la gráfica, un motor de mercado que
> entiende niveles y eventos, y un diario de trading con memoria y fundamento.
> Usuario único. Sin apuro. Bien hecho.

Estado: **planeación**. Nada de código todavía.

---

## 0. Principios (las reglas que no se negocian)

1. **Determinista primero, ML después, LLM al final.** Todo lo que se pueda calcular
   (VAL, POC, swings, rangos, rechazos) se calcula con algoritmos explícitos y auditables.
   El ML entra donde la regla escrita a mano no alcanza. El LLM sólo narra, charla y lleva
   el diario; nunca es la fuente de un número.
2. **Cada afirmación lleva fundamento rastreable.** Si el sistema dice "VAL de 80000",
   tiene que poder decir de qué rango sale, desde qué vela hasta qué vela, con qué datos,
   calculado en qué momento. Si dice "lo más probable es ir al POC", tiene que poder decir
   "en N casos históricos comparables, X% tocó el POC antes de perder el VAL".
3. **Point-in-time siempre.** Ningún nivel, evento ni feature puede usar información que no
   existía en ese momento. El sesgo de futuro (lookahead) es lo que mata a todos los
   backtests caseros. El motor se diseña para correr igual en vivo que en histórico.
4. **El motor nunca ejecuta.** Informa. Las órdenes las ponés vos.
5. **Local-first, un solo usuario.** Tus claves, tus datos, tu máquina/servidor. Nada de
   multiusuario, nada de SaaS.
6. **Las herramientas reemplazan al contexto infinito.** El LLM consulta (trades, PnL,
   eventos, niveles) mediante tools; no se le tira toda la historia en el prompt.

---

## 1. Visión general por capas

```
┌──────────────────────────────────────────────────────────────────────────┐
│ UI  — Gráfica propia · Dibujo · Indicadores · Order lines · Ticket de     │
│       futuros · Alertas · Replay · Diario · Chat                          │
├──────────────────────────────────────────────────────────────────────────┤
│ API local (WebSocket + HTTP)                                              │
├───────────────┬────────────────────┬─────────────────┬───────────────────┤
│ Ejecución     │ Alertas            │ Motor de        │ Diario + LLM      │
│ (OMS unificado│ (servidor 24/7)    │ mercado         │ (tools sobre DB)  │
│  + paper)     │                    │ niveles/eventos │                   │
├───────────────┴────────────────────┴─────────────────┴───────────────────┤
│ Market data: ingesta en vivo + histórico · normalización de símbolos      │
├──────────────────────────────────────────────────────────────────────────┤
│ Almacenamiento: series temporales (velas/trades) · Postgres (estado,      │
│ órdenes, diario) · Parquet (research/entrenamiento)                       │
├──────────────────────────────────────────────────────────────────────────┤
│ Conectores: Binance · Bybit · BingX · BloFin · MEXC · Paper               │
│ Datos tradicionales: SPY · NQ/NASDAQ · CL · BZ                            │
└──────────────────────────────────────────────────────────────────────────┘
```

Despliegue: **dos piezas**.

- **Daemon / servidor** (VPS o mini-PC en casa, prendido 24/7): ingesta de datos, motor,
  alertas, OMS, base de datos. Si las alertas y las zonas viven sólo en la app de
  escritorio, cuando cerrás la notebook se mueren. Por eso el cerebro vive en el servidor.
- **Cliente** (app de escritorio con Tauri, y el mismo front servido como web para el
  celular): pura visualización y control.
- **Notificaciones**: bot de Telegram propio (más simple y confiable que push nativo).
  Opcional: push web después.

---

## 2. Conectores a exchanges

### 2.1 Interfaz unificada

Un único contrato interno `Venue` que cada exchange implementa:

- Market data: velas, trades, libro (L2), funding, open interest, mark/index price.
- Cuenta: balances spot, posiciones de futuros, margen, modo de margen, apalancamiento.
- Órdenes: limit, market, stop, stop-limit, take-profit, trailing, OCO/brackets,
  reduce-only, post-only, modificación y cancelación.
- Streams privados: fills, cambios de órdenes, cambios de posición.

Arriba de eso, una **matriz de capacidades por exchange**: no todos soportan lo mismo
(OCO en spot, TP/SL adjuntos a la posición, trailing nativo, modificación atómica).
Cuando un exchange no soporta algo nativo, el OMS decide: o lo emula del lado del servidor
(con advertencia explícita en la UI: "esta orden vive en TU servidor, no en el exchange"),
o lo bloquea.

### 2.2 Implementación

- Base: **CCXT** sirve para arrancar rápido y como referencia, pero para ejecución seria y
  websockets se escriben **adaptadores nativos** por exchange. Las rarezas de cada API
  (precisión, tick size, step size, límites de rate, ids de cliente, firmas) se encapsulan
  ahí.
- Orden de prioridad sugerido: **Binance → Bybit → BingX → BloFin → MEXC** (de más a menos
  maduro como API). Hay que verificar al momento de implementar qué permite cada uno vía
  API para cuentas retail; MEXC en particular históricamente tuvo restricciones con órdenes
  de futuros vía API. Si un exchange no deja operar futuros por API, queda en modo
  "sólo datos + spot".
- **Reconciliación**: el OMS nunca confía en su propio estado; al reconectar, compara contra
  el exchange y corrige. Cada orden lleva un `client_order_id` propio para trazabilidad.

### 2.3 Seguridad de claves

- Claves de API **sin permiso de retiro**, siempre. Con whitelist de IP del servidor.
- Guardadas cifradas (keystore con passphrase o el keychain del sistema), nunca en texto
  plano ni en el repo.
- Separación de claves por exchange y por subcuenta.
- Modo "sólo lectura" por exchange para cuando querés mirar sin riesgo de tocar nada.

### 2.4 Paper trading

Un `Venue` más, indistinguible para el resto del sistema:

- Usa el **libro y los trades reales** de un exchange de referencia para simular fills
  (una limit se llena cuando el precio la cruza; una market consume el libro con
  slippage real).
- Simula comisiones maker/taker, funding de perpetuos, margen, y **liquidaciones**.
- Varias cuentas paper en paralelo (ej. "paper con reglas estrictas" vs "paper libre").
- El mismo simulador sirve después para el **modo replay** (sección 5.6).

---

## 3. Universo de símbolos (cero ruido)

- **Lista curada a mano.** No hay buscador global de 40.000 tickers. Agregás un par a tu
  universo y listo. Lo que no está en tu universo no existe.
- **Activo canónico vs instrumento de venue.** Un activo canónico (`BTC/USD`) se mapea a
  instrumentos concretos: `BINANCE:BTCUSDT` spot, `BINANCE:BTCUSDT` perp,
  `BYBIT:BTCUSDC` perp, `BITSTAMP:BTCUSD` (para histórico), etc.
- La gráfica puede mostrar un instrumento concreto (para operar) o el **compuesto
  canónico** (para análisis), y el motor trabaja sobre el canónico.
- Etiquetas propias: "majors", "los que opero", "macro", etc.

### Sobre "la diferencia de precio entre exchanges no importa"

Correcto **para el precio**: 89976 vs 89973 es ruido. **No es correcto para el volumen.**
El volumen de Binance no es el de Coinbase, el de spot no es el de perpetuos, y USDT vs
USDC tiene liquidez muy distinta. Como VAL/POC/VAH salen del perfil de volumen, hay que
decidir explícitamente qué volumen se usa:

- **Opción A — venue de referencia:** el perfil se arma con el volumen de un solo venue
  (ej. Binance spot BTCUSDT desde 2017). Simple y consistente.
- **Opción B — compuesto agregado:** suma de volumen de varios venues normalizados.
  Más representativo, más laburo, y hay que cuidar que el set de venues no cambie a lo
  largo del tiempo (si no, el perfil "salta" cuando entra un exchange nuevo).

Recomendación: **A para empezar** (Binance spot como referencia cripto), B como mejora.
El precio puede venir del compuesto; el volumen, del venue de referencia, y eso queda
registrado en el fundamento de cada nivel.

---

## 4. Datos históricos

"Si los datos probablemente los tenga todos, no?" — Casi. Esto es lo que hay:

### 4.1 Cripto

| Fuente | Qué | Desde |
|---|---|---|
| Bitstamp | BTCUSD trades/velas | 2011 |
| Coinbase | BTC-USD, ETH-USD trades/velas | 2015 / 2016 |
| Binance (dumps públicos) | velas, aggTrades, trades spot y futuros | 2017 (spot), 2019 (futuros) |
| Bybit (dumps públicos) | trades de derivados | ~2019-2020 |
| Funding / OI | APIs de cada exchange | variable, OI histórico suele ser corto |

Plan: **BTCUSD desde 2011** (Bitstamp + Coinbase) para el precio de largo plazo;
**Binance desde 2017** como volumen de referencia con resolución de trade.
ETH desde 2015-2016. Esto alcanza de sobra para entrenar.

### 4.2 Mercado tradicional

- **SPY**: ETF, datos intradía accesibles (proveedores tipo Polygon/Databento/FirstRate).
- **NASDAQ**: mejor usar **NQ (futuros del Nasdaq-100)** o QQQ; el índice en sí no tiene
  volumen propio.
- **CL (WTI)** y **BZ/Brent**: futuros. Requieren **contratos continuos con ajuste por
  roll** (back-adjusted), si no aparecen gaps falsos cada vencimiento que el motor
  confundiría con estructura. El volumen se toma del contrato front.
- Datos intradía de futuros históricos **cuestan plata** (CME/ICE vía Databento o similar).
  Es el único gasto relevante del proyecto. Se puede arrancar con diario y 1h gratis y
  comprar intradía cuando el motor tradfi lo justifique.
- Sesiones: el mercado tradicional tiene horario (RTH vs ETH), gaps de fin de semana,
  feriados. El motor tradfi tiene que entender sesiones; el cripto no. Por eso son
  modelos separados (como planteaste) y además **pipelines de features separados**.

### 4.3 Almacenamiento

- **Trades crudos y velas**: base de series temporales (ClickHouse o QuestDB) para
  consultas rápidas desde la gráfica y el motor.
- **Parquet particionado** (por símbolo/día) como copia fría para research y entrenamiento.
- **Postgres** para todo lo transaccional: órdenes, fills, alertas, dibujos, diario,
  niveles detectados, eventos.
- Velas de cualquier temporalidad se derivan de trades/1m; nada de confiar en velas
  pre-agregadas de terceros sin validar.
- **Control de calidad de datos**: detección de huecos, velas duplicadas, prints absurdos
  (flash crashes de un solo exchange), cambios de tick size. Cada dataset tiene un reporte
  de calidad.

---

## 5. La gráfica

### 5.1 Motor de renderizado

Recomendación: **renderer propio** (Canvas 2D primero, WebGL para lo pesado).

Por qué no una librería existente:
- Lightweight Charts no trae herramientas de dibujo y el plugin API queda corto para
  perfiles de volumen, footprint y overlays del motor.
- La librería avanzada de TradingView tiene licencia restrictiva y no la controlás.
- Con tiempo infinito, controlar el renderer es lo que permite que niveles del motor,
  perfiles, órdenes y eventos sean ciudadanos de primera clase.

Requisitos: millones de velas con scroll/zoom fluido (LOD / decimación), escala log y
lineal, multi-panel, multi-gráfica sincronizada (mismo cursor/tiempo en 4 layouts),
temas, atajos de teclado a lo TradingView para no reaprender.

### 5.2 Herramientas de dibujo

Primera tanda: línea horizontal, rayo horizontal, línea de tendencia, rectángulo/zona,
canal paralelo, Fibonacci retroceso/extensión, rango de precio/fecha (medidor),
texto/nota, posición larga/corta (calculadora R:R visual), **perfil de volumen anclado
dibujable** (elegís desde-hasta y te dibuja VAH/POC/VAL), VWAP anclado.

Todo dibujo:
- Se guarda en servidor, atado al **activo canónico** (lo dibujás en BTCUSDT de Binance y
  aparece en BTCUSDC de Bybit).
- Se puede convertir en **alerta** con un click.
- Se puede convertir en **orden** si es una línea horizontal (sección 6).
- Puede marcarse como "nivel mío" para que el motor lo trate como un nivel más y te avise
  eventos sobre él (rechazo, ruptura, aceptación).

### 5.3 Indicadores

- Los clásicos (EMA/SMA, RSI, MACD, BB, ATR, VWAP, volumen, OBV, etc.).
- Los de volumen que importan para tu forma de operar: **perfil de volumen** (visible range,
  sesión, fijo, anclado), **VWAP anclados**, CVD (delta acumulado), footprint/clusters
  (más adelante, necesita trades).
- Derivados: funding, open interest, liquidaciones, basis spot-perp.
- **Lenguaje de indicadores propio** (más adelante): un DSL o scripting embebido (ej. Lua o
  un subset tipo Pine) que corre igual en la gráfica, en las alertas y en el backtest.
  Un único motor de cálculo para los tres lugares; si no, lo que ves y lo que te alerta
  divergen.

### 5.4 Overlays del motor

Capas activables:
- Niveles detectados (con color por tipo y opacidad por fuerza/confluencia).
- Rangos detectados (caja con VAH/POC/VAL internos).
- Eventos (marcadores: toque, rechazo, ruptura, aceptación, retest) — click en el
  marcador abre el **fundamento completo**.
- Conteos de Elliott candidatos (fase final del proyecto).

### 5.5 Alertas

- Viven en el **servidor**, evaluadas sobre el stream de trades (no sobre velas cerradas,
  salvo que la alerta lo pida).
- Tipos: cruce de precio, cruce de línea/tendencia, entrada/salida de zona, condición de
  indicador, **evento del motor** ("rechazo en cualquier VAL de 4h+"), estado de cuenta
  (margen, PnL diario, distancia a liquidación).
- Condiciones con cierre de vela opcional ("sólo si cierra 1h por debajo").
- Canal: Telegram + UI. Con anti-spam (cooldown, una vez/cada vez).

### 5.6 Replay

Reproducir el mercado vela a vela (o trade a trade) desde cualquier fecha, con el motor
corriendo point-in-time y la cuenta paper activa. Sirve para tres cosas:
1. Practicar sin quemar plata.
2. **Validar visualmente que el motor no mira el futuro** (si en replay detecta algo que
   "no podía saber", hay un bug).
3. Revisar trades del diario viendo exactamente qué había en pantalla en ese momento.

---

## 6. Trading en la gráfica (spot)

Tu caso: "si BTC llega a 75k, compro. Me chupa un huevo cómo llegue."

### 6.1 Order lines

- Arrastrás una línea horizontal → "Comprar acá" → ticket corto: monto (en USD o en %
  del saldo disponible), exchange, par. Confirmás. Listo.
- La línea queda dibujada con estado: **pendiente / parcialmente llena / llena /
  cancelada**. Arrastrarla modifica la orden. Click derecho → cancelar.
- Lo mismo para venta.

### 6.2 Dónde vive la orden (decisión importante, explícita en la UI)

- **Limit nativa en el exchange** (default para spot): la orden está en el libro de
  Binance; aunque tu servidor se caiga, se ejecuta. Contra: el saldo queda bloqueado.
- **Trigger en servidor**: tu servidor mira el precio y manda la orden cuando toca. No
  bloquea saldo y permite lógica (ej. "si cierra 4h arriba de X"). Contra: si el servidor
  está caído, no pasa nada.

La línea muestra un ícono distinto según cuál sea. Para "la compro sí o sí a 75k", limit
nativa.

### 6.3 Escaleras (ladders)

Una **zona** (rectángulo) en vez de una línea: "comprar 10.000 USD repartidos en 5 órdenes
entre 78k y 74k", distribución lineal o cargada abajo. Cada sub-orden es una línea visible.

### 6.4 Planes de acumulación

Zonas de compra persistentes de largo plazo agrupadas en un "plan" (ej. "Plan acumulación
BTC 2026"): cuánto capital tiene asignado, cuánto se ejecutó, precio promedio resultante,
qué queda pendiente.

---

## 7. Ticket de futuros (UX decente, no la de Binance)

No se hace con líneas: se hace con un panel bien pensado.

### 7.1 Flujo

1. **Dirección**: Long / Short (dos botones grandes, colores claros).
2. **Entrada**: market, limit (precio), o escalera (rango + cantidad de órdenes).
3. **Stop loss**: precio. Obligatorio por defecto (se puede desactivar en ajustes,
   pero con fricción a propósito).
4. **Riesgo**: en USD o en % del capital. **El tamaño se calcula solo** a partir de la
   distancia entrada-SL. Vos no pensás en "contratos" ni en "apalancamiento": pensás en
   cuánto estás dispuesto a perder.
5. **Take profits**: uno o varios (TP1 50% en 2R, TP2 30% en 3R, runner con trailing),
   en precio o en R.
6. **Gestión automática opcional**: mover SL a break-even al tocar TP1, trailing, etc.
7. **Apalancamiento y modo de margen**: se calculan/sugieren a partir de lo anterior
   (el apalancamiento mínimo que permite esa posición con margen holgado).

### 7.2 Preview antes de confirmar

- Tamaño, nocional, margen usado, **precio de liquidación** y distancia al SL
  (si el SL está más allá de la liquidación → bloqueo con explicación).
- R:R por TP, PnL esperado en cada TP, comisiones estimadas, funding estimado si se
  mantiene X horas.
- La posición se dibuja en la gráfica (como la herramienta "posición larga" de TV) antes
  de enviar, y podés ajustar arrastrando entrada/SL/TP en ese preview.

### 7.3 Plantillas

Guardar setups recurrentes ("Scalp 0.5% riesgo, TP 1.5R/3R, BE en TP1"). Un click carga
el ticket.

### 7.4 Ejecución

- El OMS manda entrada + SL + TPs como **bracket atómico** si el exchange lo soporta; si
  no, en secuencia con verificación y rollback (si el SL falla en colocarse, se cierra la
  posición y se avisa). **Nunca una posición sin SL por un error silencioso.**
- Posición abierta visible en la gráfica con líneas arrastrables de SL/TP.

### 7.5 Guardarraíles opcionales (porque "sabés cómo soy")

Autoimpuestos, configurables, desactivables con fricción (ej. esperar 10 minutos):
- Pérdida diaria/semanal máxima → bloqueo de nuevas entradas.
- Cooldown después de una pérdida.
- Máximo de trades por día.
- Riesgo máximo por trade.
- El diario registra cada vez que los desactivás (y el LLM te lo va a recordar).

---

## 8. El motor de mercado (el corazón)

Acá está el valor real del proyecto. Y sí: **para avisarte que el precio llegó al VAL y
rechazó, no necesitás un LLM.** Necesitás: motor de mercado → detector de eventos →
estadística histórica → plantilla de texto. El LLM, si está, sólo pone linda la prosa.

### 8.1 Pipeline

```
Datos (trades/velas, point-in-time)
  │
  ▼
[L1] Features base          swings multi-TF, ATR, volumen relativo, delta, VWAPs,
                            perfiles de volumen
  │
  ▼
[L2] Registro de niveles    cada nivel con tipo, precio, origen, fuerza, vigencia
  │
  ▼
[L3] Detector de eventos    máquinas de estado por nivel: aproximación → toque →
                            rechazo | aceptación | ruptura → retest
  │
  ▼
[L4] Confluencias           qué otros niveles/contextos coinciden en esa zona
  │
  ▼
[L5] Estadística/ML         qué pasó históricamente en eventos comparables
  │
  ▼
[L6] Narrador               plantilla determinista (+ LLM opcional para redactar)
  │
  ▼
Alerta / overlay / diario
```

### 8.2 [L2] Tipos de nivel

Cada uno es un módulo independiente (los "modelos distintos" que planteaste, pero la
mayoría son algoritmos, no redes):

- **Perfil de volumen** — VAH / POC / VAL para:
  - sesiones (día, semana, mes),
  - **rangos detectados** (el caso de tu ejemplo),
  - anclajes desde swings relevantes ("desde el máximo de 93000 hasta ahora"),
  - nodos de alto/bajo volumen (HVN/LVN) y POCs vírgenes (naked POC).
- **Soportes y resistencias** por swings: clusters de pivots multi-temporalidad, con
  cantidad de toques, edad, reacción promedio, si fueron rotos y recuperados (flip).
- **Rangos / áreas**: detección de consolidaciones (compresión de volatilidad, precio
  oscilando entre extremos con aceptación), con sus bordes y su perfil interno.
- **VWAPs anclados** desde eventos (máximos/mínimos mayores, inicios de mes, etc.).
- **Niveles redondos y psicológicos** (75k, 80k, 100k).
- **Fibonacci** desde swings mayores.
- **Niveles tuyos** (los que dibujás y marcás).
- **Ondas de Elliott** (sección 8.7, lo último).

Cada nivel guarda su **procedencia completa**:

```
nivel: VAL
precio: 80012
tipo_perfil: rango anclado
ancla_inicio: 2026-05-10 10:00 UTC (máximo en 93000, temporalidad 1D)
ancla_fin: abierto (se actualiza)
fuente_volumen: Binance spot BTCUSDT, trades
calculado_en: <timestamp point-in-time>
temporalidad: alta
toques_previos: 0
```

Con eso, la frase de tu ejemplo se arma sola.

### 8.3 [L3] Eventos — definiciones operativas

La parte fina es definir **sin ambigüedad** qué es cada cosa, porque después todo se
entrena y se mide contra estas definiciones. Ejemplos (los umbrales se calibran, todos
relativos a ATR para que funcionen en cualquier activo y época):

- **Toque**: el precio entra en una banda de ±k·ATR alrededor del nivel.
- **Rechazo**: tocó, la vela (en la TF del nivel o una menor configurada) cierra de vuelta
  del lado original, con mecha ≥ x% del rango de la vela, y no hay aceptación del otro
  lado (no hay N cierres seguidos del otro lado).
- **Aceptación**: N cierres del otro lado + volumen negociado del otro lado ≥ umbral.
- **Ruptura**: aceptación con desplazamiento ≥ m·ATR.
- **Retest**: vuelta al nivel roto desde el otro lado.
- **Calidad del evento**: volumen relativo en el toque, delta (¿hubo absorción?),
  velocidad de llegada, cuántas veces fue tocado antes (primer toque vs enésimo).

Tu frase "recomiendo esperar confirmación por falta de volumen en el rechazo" sale de
la **calidad del evento**: volumen relativo bajo en la vela de rechazo.

Todas las definiciones son versionadas: si cambio la definición de "rechazo", la versión
nueva se re-corre sobre todo el histórico y las estadísticas se recalculan.

### 8.4 [L4] Confluencias

Para cada zona de precio: qué niveles de qué tipos y temporalidades coinciden dentro de
una tolerancia (en ATR). Score de confluencia = combinación ponderada, **con pesos
aprendidos** (en L5) de qué combinaciones históricamente importaron, no pesos inventados.

### 8.5 [L5] Estadística y ML — de dónde sale "lo más probable"

Esta es la diferencia entre fundamento y horóscopo.

1. **Estudio de eventos histórico**: se corre el motor point-in-time sobre todo el
   histórico (BTC desde 2011, etc.) y se guarda **cada evento** con su contexto
   (features) y **qué pasó después**.
2. **Etiquetado por resultado** (sin anotación manual): para cada evento, qué tocó primero
   dentro de un horizonte — el objetivo (ej. POC), la invalidación (ej. pérdida del VAL
   por k·ATR) o ninguno (método de "triple barrera").
3. **Tasas base condicionadas**: "rechazo en VAL de rango de alta TF, primer toque, volumen
   bajo → N=214, 58% tocó POC antes de invalidar, mediana 31h". Esto **ya es útil sin ML**.
4. **Modelos** (cuando hay suficientes eventos): gradient boosting sobre las features del
   evento, con **probabilidades calibradas** (si dice 60%, que pase 60% de las veces).
   Validación walk-forward con purga (nunca mezclar futuro en el entrenamiento).
5. Siempre se reporta **N y el intervalo de confianza**. "70% con N=12" se muestra como lo
   que es: poco confiable.

Modelos separados, como pediste:
- por **mercado**: cripto vs tradicional (comportamientos y sesiones distintas);
- por **tipo de evento/nivel**: S/R, perfiles de volumen/rangos, confluencias.
  Cada uno tiene su dataset, su validación y su reporte de performance.

### 8.6 ¿Y la LoRA sobre un 4B/9B para detectar patrones?

Opinión honesta: **para detectar patrones en series de precios, un LLM es la herramienta
equivocada.** Los LLMs son malos con secuencias numéricas largas, caros de correr sobre
cada tick, y difíciles de calibrar. Para lo que querés:

- Lo detectable con reglas (VAL, POC, swings, rechazos) → **algoritmos** (L1–L3).
- Lo "difuso" (¿esto es un rango limpio o basura? ¿este rechazo es de calidad?) →
  **modelos chicos de series temporales** (gradient boosting sobre features, o una red
  1D-CNN/transformer chico sobre ventanas normalizadas). Entrenan en minutos/horas, corren
  en milisegundos, se calibran.
- Etiquetas: auto-generadas por resultado (8.5) + **anotaciones tuyas** desde la UI
  ("esto es un rango", "esto fue un rechazo válido") para los casos subjetivos.

El LLM con LoRA sí tiene sentido para **lenguaje**: diario y charla (sección 9).

### 8.7 Ondas de Elliott

Lo último, a propósito: es lo más subjetivo y lo más fácil de sobreajustar.
- Generador de **conteos candidatos** sobre swings multi-grado, filtrado por las reglas
  duras (onda 2 no retrocede más del 100% de la 1, onda 3 no es la más corta, onda 4 no
  se superpone con la 1 en impulsos), puntuado por guías (proporciones Fibonacci,
  alternancia, canalización).
- Se muestran **varios conteos con su puntaje**, nunca "el" conteo.
- Un modelo aprende a rankear conteos según qué tan bien predijeron después.
- Se mide igual que todo lo demás: si no aporta sobre la tasa base, se dice.

### 8.8 [L6] Narrador

Plantillas deterministas alimentadas por el evento + niveles + estadística:

> El precio llegó a **80012** (VAL de rango de alta temporalidad, anclado desde el
> máximo de **93000** del 10/05/2026 10:00 hasta ahora). **Primer toque. Rechazo** en 4h
> con volumen relativo **0.6×** (bajo). Históricamente, en rechazos comparables (N=214),
> el **58%** buscó el **POC (84000)** antes de perder el VAL; mediana 31h.
> Volumen bajo en el rechazo: **esperar confirmación**.

Sin LLM. Exacto, reproducible, auditable. El LLM, si se usa acá, sólo reescribe la prosa
y **no puede introducir números** (se valida que todo número del texto final exista en
los datos de entrada; si no, se descarta y se usa la plantilla).

---

## 9. Diario de trading + LLM

### 9.1 El diario (sin LLM ya es muy útil)

Cada trade (real o paper) se registra automáticamente con:
- datos de ejecución: entradas, salidas, tamaños, comisiones, funding, PnL, **R-múltiplo**;
- **snapshot del motor en el momento de la entrada**: niveles cercanos, eventos recientes,
  score de confluencia, probabilidad histórica del contexto;
- captura de la gráfica;
- campos tuyos: setup (etiqueta), razón, plan, estado emocional, nota post-trade;
- flags automáticos: entró sin SL, movió el SL en contra, operó durante cooldown,
  desactivó un guardarraíl, entró sin evento del motor, trade dentro de X minutos después
  de una pérdida (revenge), sobre-tamaño respecto de su riesgo habitual.

Estadísticas: expectativa, win rate y R promedio por setup, por hora, por día, por
activo, por exchange, por "con/sin confluencia", curva de capital, drawdown, evolución
mensual ("cuánto mejoraste").

### 9.2 El LLM (4B local, con tools)

- Modelo chico local (clase Qwen/Gemma/Llama de 4B; 9B si hay GPU) servido con
  llama.cpp / Ollama / vLLM.
- **No tiene el historial en contexto. Tiene herramientas**:
  - `get_trades(filtros)`, `get_trade(id)`, `get_stats(filtros, agrupación)`
  - `get_equity_curve(rango)`, `get_account_state()`
  - `get_engine_snapshot(símbolo, timestamp)` — qué veía el motor en ese momento
  - `get_events(símbolo, rango, tipo)`, `get_levels(símbolo, timestamp)`
  - `search_journal(texto)` — búsqueda semántica sobre tus notas
  - `get_flags(rango)` — errores detectados
  - `get_rules()` — tus guardarraíles y si los cumpliste
- Usos:
  - "¿Por qué perdí plata en septiembre?" → consulta stats por setup, encuentra que el 70%
    de las pérdidas fueron entradas sin evento del motor después de las 22hs.
  - "Revisá este trade" → trae snapshot del motor, compara con tu razón declarada.
  - Resumen semanal automático del diario.
  - Charlar sobre el mercado **con el motor como fuente**, no con "lo que el modelo cree".
- **Regla dura**: el LLM no recomienda entradas ni da señales. Explica, resume, revisa.
- **LoRA**: el valor no está en enseñarle trading (eso lo pone el motor), sino en
  (a) que use bien las tools, en tu formato, y (b) que hable tu idioma y tu jerga.
  Dataset: conversaciones sintéticas generadas sobre tus propios datos + correcciones tuyas.
  Se evalúa con un set fijo de preguntas con respuestas verificables contra la DB.

---

## 10. Stack técnico propuesto

| Pieza | Elección | Motivo |
|---|---|---|
| Daemon (market data, OMS, alertas, motor en vivo) | **Rust** | latencia predecible, sin GC, seguridad de memoria para lo que toca plata |
| Research, entrenamiento, backtests masivos | **Python** (Polars, NumPy, LightGBM, PyTorch) | ecosistema |
| Motor de features/niveles/eventos | **Rust, compartido** (expuesto a Python vía PyO3) | **el mismo código corre en vivo y en histórico**: cero divergencia |
| Front | TypeScript + renderer propio (Canvas/WebGL) | control total |
| Desktop | Tauri | liviano, mismo front |
| Series temporales | ClickHouse (o QuestDB) | consultas rápidas sobre miles de millones de trades |
| Transaccional | Postgres | órdenes, diario, niveles, eventos |
| Research | Parquet | portable, rápido |
| LLM | llama.cpp / Ollama local | privado, gratis |
| Notificaciones | Telegram bot | simple, confiable |

El punto más importante de la tabla: **el motor es una sola librería** usada por el
daemon en vivo, por el backtester, por el replay y por el pipeline de entrenamiento.
Si hay dos implementaciones, tarde o temprano dicen cosas distintas.

---

## 11. Roadmap por fases (ordenado por dependencias, no por tiempo)

**Fase 0 — Cimientos**
- Monorepo, convenciones, CI, tests.
- Modelo de dominio: activo canónico, instrumento, venue, orden, fill, posición, nivel,
  evento.
- Keystore cifrado.

**Fase 1 — Datos**
- Ingesta histórica: Bitstamp/Coinbase BTCUSD, Binance dumps (BTC, ETH).
- Ingesta en vivo Binance (trades, libro, velas).
- Almacenamiento + control de calidad + agregación de velas.

**Fase 2 — Gráfica base**
- Renderer: velas, volumen, zoom/scroll, escalas, multi-panel, crosshair.
- Universo curado de símbolos.
- Dibujo básico (horizontal, tendencia, rectángulo, Fibo) persistido en servidor.
- Indicadores clásicos + perfil de volumen + VWAP anclado.

**Fase 3 — Alertas**
- Evaluador en servidor sobre stream de trades.
- Alertas de precio/línea/zona/indicador → Telegram.

**Fase 4 — Paper trading + ejecución spot en la gráfica**
- Venue paper con fills realistas.
- Order lines spot (limit nativa / trigger servidor), ladders, planes de acumulación.
- Conector Binance real (spot).

**Fase 5 — Futuros**
- Ticket de futuros con sizing por riesgo, brackets, preview de liquidación, plantillas.
- Conector Binance futuros; luego Bybit, BingX, BloFin, MEXC (capacidades verificadas).
- Guardarraíles.

**Fase 6 — Diario (sin LLM)**
- Registro automático, flags, estadísticas, capturas.

**Fase 7 — Motor v1 (determinista)**
- L1 features, L2 niveles (perfiles, S/R, rangos, VWAP, redondos, propios),
  L3 eventos con definiciones versionadas.
- Overlays en la gráfica + alertas por evento del motor.
- Snapshot del motor en el diario.

**Fase 8 — Estudio histórico y estadística**
- Backtest point-in-time del motor sobre todo el histórico.
- Etiquetado por resultado, tasas base condicionadas, reportes.
- Narrador por plantillas con estadística → el mensaje de tu ejemplo, completo.
- Replay.

**Fase 9 — ML**
- Modelos calibrados por mercado y por tipo de evento.
- Anotación manual desde la UI para casos subjetivos.
- Score de confluencia aprendido.

**Fase 10 — Mercado tradicional**
- Datos SPY/NQ/CL/BZ (continuos con roll), sesiones.
- Pipeline y modelos tradfi separados.

**Fase 11 — LLM**
- Servidor local, tools sobre la DB, chat en la app, resúmenes del diario.
- Dataset y LoRA; set de evaluación.

**Fase 12 — Elliott + extras**
- Conteos candidatos, ranking aprendido.
- DSL de indicadores, footprint, CVD, liquidaciones, heatmap de libro.

---

## 12. Riesgos y trampas conocidas

- **Lookahead bias** en niveles y rangos (un rango "detectado" usando velas que todavía no
  existían). Mitigación: motor único point-in-time + replay + tests que comparan
  "corrida incremental vela a vela" vs "corrida en batch" (tienen que dar idéntico).
- **Sobreajuste** de umbrales de eventos. Mitigación: walk-forward, pocos parámetros,
  todo relativo a ATR, reportar N siempre.
- **Cambio de régimen**: BTC 2013 no es BTC 2026. Mitigación: features normalizadas,
  ponderar datos recientes, reportar estadísticas por época.
- **Órdenes huérfanas / posición sin SL** por fallas de red. Mitigación: reconciliación,
  brackets atómicos, rollback, alertas de estado.
- **APIs que cambian o restringen** (sobre todo los exchanges más chicos). Mitigación:
  adaptadores aislados, matriz de capacidades, tests contra testnet donde exista.
- **Seguridad**: claves sin retiro + whitelist IP + cifrado. El servidor expuesto a
  internet sólo por VPN (Tailscale/WireGuard), nunca con puertos abiertos.
- **Usarlo como señal aunque no lo sea**. Mitigación: lenguaje del narrador siempre en
  términos de "históricamente" con N, y el diario midiendo si seguir o ignorar el motor
  te fue mejor o peor.

---

## 13. Preguntas abiertas (para decidir antes de arrancar)

1. **Dónde corre el servidor**: ¿VPS (latencia baja a los exchanges, ~5-20 USD/mes) o
   mini-PC en casa?
2. **¿Tenés GPU?** ¿Cuál y cuánta VRAM? Define 4B vs 9B y si la LoRA se entrena local o
   alquilando GPU por horas.
3. **Temporalidades que operás** y qué considerás "larga temporalidad" (¿1D? ¿1W?).
4. **Tu definición de rango/área**: ¿cómo lo marcás vos hoy? (Es la base para la
   detección automática y para las anotaciones.)
5. **Volumen de referencia**: ¿Binance spot te representa, o preferís compuesto desde el
   inicio?
6. **Celular**: ¿querés operar desde el celular o sólo recibir alertas y mirar?
7. **Presupuesto de datos tradfi**: ¿arrancamos con diario/1h gratis y compramos intradía
   de futuros más adelante?
8. **Exchanges reales hoy**: ¿cuáles usás de verdad ahora? Esos van primero.
