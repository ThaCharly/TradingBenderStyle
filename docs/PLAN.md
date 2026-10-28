# TradingBenderStyle — Plan maestro

> Un TradingView personal, sin ruido, con ejecución en la gráfica, un motor de mercado que
> entiende niveles y eventos, y un diario de trading con memoria y fundamento.
> Usuario único. Sin apuro. Bien hecho.

Estado: **planificación cerrada**; pasa a ejecución ([TRASPASO](TRASPASO.md)). Versión del plan: **v3** (después de la [revisión arquitectónica](revision/2026-10-27-revision-arquitectonica.md)).
Nada de código todavía.

Mapa de documentos:
- Arquitectura: [00 estructura del repo](arquitectura/00-estructura-del-repo.md) ·
  [01 modelo de datos](arquitectura/01-modelo-de-datos.md) ·
  [02 motor reproducible](arquitectura/02-motor-historico-reproducible.md) ·
  [03 análisis/ejecución](arquitectura/03-separacion-analisis-ejecucion.md) ·
  [04 probabilidades](arquitectura/04-probabilidades-auditables.md) ·
  [05 núcleo temporal](arquitectura/05-nucleo-temporal.md) ·
  [06 ejecución de futuros](arquitectura/06-ejecucion-futuros.md) ·
  [07 topología](arquitectura/07-topologia-distribuida.md) ·
  [08 aislamiento del LLM](arquitectura/08-aislamiento-llm.md) ·
  [09 MCP de análisis](arquitectura/09-mcp-de-analisis.md)
- Decisiones: [ADRs](decisiones/README.md)
- Resumen para el dueño: [FUNCIONALIDADES](FUNCIONALIDADES.md)
- Trabajo: [PROTOTIPO inicial](PROTOTIPO.md) · [ROADMAP](ROADMAP.md) (milestones e issues) · [guía para agentes](AGENTES.md) · [estrategia de ramas](RAMAS.md) · [archivado vs respaldo](operacion/archivado-vs-respaldo.md)

---

## 0. Los cuatro pilares

Todo el diseño se subordina a estos cuatro. Cada uno tiene su documento:

1. **[Modelo de datos universal](arquitectura/01-modelo-de-datos.md)** — una orden, una
   posición, una ejecución y un evento de mercado tienen la misma representación interna
   en todos los exchanges. Las rarezas mueren en el adaptador.
2. **[Motor histórico reproducible](arquitectura/02-motor-historico-reproducible.md)** —
   se puede volver a cualquier instante (ej. 10/05/2026 10:00) y reconstruir exactamente
   qué se sabía. Base para entrenar y evaluar sin contaminación del futuro.
3. **[Separación absoluta análisis / ejecución](arquitectura/03-separacion-analisis-ejecucion.md)**
   — detectores, modelos y LLM no tienen permisos de operar. Sólo `execution` envía
   órdenes, y sólo por comandos firmados desde tus dispositivos.
4. **[Probabilidades calibradas y auditables](arquitectura/04-probabilidades-auditables.md)**
   — cada porcentaje muestra sus casos, período, N y validación. Si no puede, no se
   muestra.

Y los principios que se derivan:

- **Determinista primero, ML después, LLM al final.** El LLM nunca es fuente de un número.
- **Cada afirmación lleva fundamento rastreable** (procedencia + manifiesto).
- **Local-first, un solo usuario.**
- **Herramientas en lugar de contexto infinito** para el LLM.

---

## 1. Decisiones tomadas

| Tema | Decisión |
|---|---|
| Servidor | **Mini-PC en casa**, 24/7, con **IP fija** del proveedor; disco **~1 TB** (ampliable; VPS o disco externo si falta) |
| Desarrollo | **PC local** con el perfil `dev-all` (todo en una máquina, en paper); la mini-PC entra cuando haga falta 24/7 |
| Universo inicial | **BTC y ETH** (spot + perps); tradfi **si es gratis** de poner (ver §4.2 e issue del inventario) |
| GPU | RTX 5050 8 GB (corre 4B, 9B y MoE 35B a 20–25 t/s) |
| LLM en el celular | Samsung S25 (Snapdragon 8 Elite, 12 GB) → **4B en el dispositivo** |
| Temporalidades | De 1 minuto a 1 año (segundos sólo dentro de la ventana de trades o con el perfil al tick, [ADR-0014](decisiones/ADR-0014-perfiles-de-grabacion-y-resolucion-base.md)). Uso principal: **4H y 30m**; al menos 2m y 5m |
| Rangos y canales | Desde el punto actual hacia atrás hasta donde **empieza el canal**. Los canales inclinados existen pero **no** son lo mismo que los rangos de área. **Las definiciones de análisis técnico se cierran más adelante** (§8.1) |
| Volumen | **Multi-venue, spot y perps**: Binance, BloFin, BingX, MEXC, Bybit… |
| Dispositivos | **Operar, alertas y mirar en PC y celular** por igual |
| Exchanges | Real hoy: **Binance**. Después: **BloFin** (ya tenés), **BingX**, y quizás **MEXC/Bybit** |
| Tradfi | Indefinido → propuesta en §4.2 |

---

## 2. Visión general

```
┌──────────────────────────── Clientes ────────────────────────────┐
│  PC (Tauri)                         Celular (Tauri Android)       │
│  gráfica · dibujo · ticket ·        lo mismo, táctil +            │
│  alertas · replay · diario · chat   LLM 4B local                  │
└───────────────┬───────────────── Tailscale (VPN) ─────────────────┘
                │ comandos de trading firmados por dispositivo
┌───────────────▼──────────── Mini-PC (casa, 24/7) ─────────────────┐
│  api ── sólo lectura + reenvío de comandos firmados               │
│                                                                   │
│  recorder        runner/engine    query/journal    execution      │
│  (ingesta +      (niveles,        (sólo lectura;   (ÚNICO con     │
│   grabador)       eventos,         notas del LLM)   claves; OMS,  │
│                   alertas)                          paper, trig.) │
│                                                                   │
│  Parquet + DuckDB (datos) · Postgres (estado/diario)               │
└───────────────────────────────────────────────────────────────────┘
        │ datos públicos                         │ privado (sólo execution)
        ▼                                        ▼
 Binance · BloFin · BingX · MEXC · Bybit (spot + perps)
```

La estación (PC con GPU) corre `worker` (backtests, entrenamiento) y el LLM
grande. Cada componente puede moverse de nodo por configuración
([doc 07](arquitectura/07-topologia-distribuida.md)).

### 2.1 Infraestructura en casa: lo que hay que resolver

- **Acceso remoto**: Tailscale (WireGuard). Ningún puerto abierto a internet.
- **Cortes de luz**: UPS para la mini-PC y el router. Arranque automático al volver la
  luz, con `execution` esperando tu desbloqueo (passphrase) o usando TPM.
- **Cortes de internet**: con el servidor en casa, este riesgo es real. Consecuencias:
  - Para zonas de compra spot "sí o sí", **orden limit nativa en el exchange** (vive en
    Binance aunque tu casa se quede sin internet).
  - Los triggers de servidor se marcan con advertencia y, opcionalmente, al detectar
    corte prolongado se notifica desde un watchdog externo (ver abajo).
  - SL/TP de futuros **siempre nativos** en el exchange.
- **IP fija**: decidido, **IP fija del proveedor** (whitelist de IP en las claves de
  trading). El VPS de salida queda sólo como plan B si la IP fija cambia o falla.
- **Watchdog externo**: un chequeo de vida liviano (el mismo VPS o un servicio gratuito)
  que te avisa por Telegram si la mini-PC dejó de responder.

---

## 3. Conectores a exchanges

Detalle del modelo en el [Pilar 1](arquitectura/01-modelo-de-datos.md).

- **Orden**: Binance (spot + USDⓈ-M perps) → BloFin → BingX → MEXC / Bybit.
- **Datos vs trading** se separan: el grabador se conecta a **todos** los venues desde
  el principio (sólo datos públicos, sin claves), aunque operes sólo en Binance.
- Adaptadores nativos por exchange (CCXT como referencia, no como base), con matriz de
  capacidades y suite de conformidad común.
- Verificar en cada uno qué permite la API para cuentas retail (MEXC tuvo restricciones
  históricas en órdenes de futuros por API). Si no se puede operar, queda "sólo datos".
- **Seguridad**: claves sin permiso de retiro, whitelist de IP, descifradas sólo dentro de
  `execution` ([Pilar 3](arquitectura/03-separacion-analisis-ejecucion.md)).
- **Paper trading**: un venue más, que pasa la misma suite. Fills contra libro y trades
  reales, comisiones, funding, liquidaciones. Varias cuentas paper en paralelo.

---

## 4. Datos

### 4.1 Cripto: multi-venue, spot y perps

> **Actualizado por [ADR-0014](decisiones/ADR-0014-perfiles-de-grabacion-y-resolucion-base.md)**: perfiles de grabación (liviano por defecto, al tick
> opcional), base de 1 minuto con resúmenes por banda de precio, historia descargada por el
> usuario de la fuente oficial y términos de uso por dato. Donde esta sección contradiga a
> la ADR, manda la ADR.

**Grabador en vivo desde la Fase 1**, de todos los venues: trades, libro (L2 parcial),
mark/index, funding, open interest, liquidaciones. Cada día grabado es historia perfecta
para el [Pilar 2](arquitectura/02-motor-historico-reproducible.md).

Histórico descargable (reconstruido, `provenance = backfill`):

| Fuente | Qué | Desde |
|---|---|---|
| Bitstamp | BTCUSD trades | 2011 |
| Coinbase | BTC-USD, ETH-USD | 2015 / 2016 |
| Binance (dumps públicos) | trades spot / perps | 2017 / 2019 |
| Bybit (dumps públicos) | trades derivados | ~2019–2020 |
| BloFin, BingX, MEXC | poco o nada descargable | desde que prende el grabador |

**Volumen multi-venue — cómo se compone:**

- Se guarda **por venue y por mercado** (spot / perp), siempre en **unidades del activo
  base** (BTC), así USDT, USDC y USD son comparables.
- Los perfiles se componen al consultar, con un **set de venues elegible**:
  "spot agregado", "perps agregado", "todo", o uno solo. Por defecto: dos perfiles, spot
  y perps, porque dicen cosas distintas (demanda real vs apalancamiento). La divergencia
  entre ambos es en sí una feature del motor.
- **Cobertura variable en el tiempo**: en 2018 no existían algunos venues. Para que el
  perfil no "salte" cuando entra un exchange nuevo, cada perfil registra qué venues
  incluyó, y para estadística histórica se usan medidas relativas (volumen relativo a su
  propia media por venue) que no dependen de cuántos venues hay.
- El set de venues usado queda en la procedencia de cada nivel.

### 4.2 Mercado tradicional — propuesta

No lo operás; su rol es **datos de entrenamiento** para el modelo tradfi y **contexto
macro**. Eso simplifica mucho: no hace falta feed en vivo al principio.

**Gratis desde el día uno** (inventario en un spike de M2):
- **Perps "TradFi" listados en los exchanges que ya grabamos** (oro, índices, acciones;
  qué hay exactamente y desde cuándo hay que verificarlo por venue). Son gratis y entran
  por el mismo grabador. Pero son **proxies**: cotizan 24/7 con volumen propio del
  exchange, así que sirven para mirar y alertar, **no** como datos de entrenamiento del
  modelo tradfi.
- **Históricos diarios gratuitos** de SPY, NQ, CL y Brent como referencia visual.

1. **Prototipo**: históricos diarios gratuitos de SPY, NQ/QQQ, CL y Brent para armar el
   pipeline de sesiones y roll de futuros.
2. **Compra única de histórico intradía** (barras de 1 minuto, varios años) de NQ, ES,
   CL, Brent y SPY a un proveedor de pago por uso (tipo Databento; confirmar cobertura de
   Brent/ICE). Es el único gasto relevante del proyecto y se hace recién cuando el motor
   cripto esté validado (Fase 10).
3. **Feed en vivo**: sólo si más adelante querés alertas tradfi. Se decide entonces.

Detalles técnicos: futuros como **continuos ajustados por roll**; NASDAQ vía **NQ**
(el índice no tiene volumen propio); sesiones RTH/ETH, feriados y gaps de fin de semana.

### 4.3 Almacenamiento ([ADR-0007](decisiones/ADR-0007-almacenamiento-minimo.md))

- **Zona cruda** (Parquet/compresión, append-only, con hash) → **normalizada** →
  **derivada**, según el Pilar 2.
- **DuckDB** embebido sobre Parquet para consultas analíticas; rollups de velas en
  Parquet con caché en memoria en `query`. ClickHouse sólo si no se cumple el
  presupuesto de latencia (5.000 velas en ≤ 50 ms p95).
- **Postgres** para órdenes, eventos de trading, ledger, dibujos, alertas, diario,
  niveles y eventos del motor, leases y cola de trabajos.
- Grabación de libro L2 con **top-N configurable por venue** para acotar el disco.
- Control de calidad por dataset: huecos, duplicados, prints absurdos, cambios de spec.

Estimación de disco: el grabador multi-venue con L2 parcial genera decenas de GB por mes
comprimido. Planear un disco dedicado de varios TB.

---

## 5. La gráfica

### 5.1 Render: Lightweight Charts v5 detrás de una escena propia ([ADR-0001](decisiones/ADR-0001-motor-grafico.md))

- **`ChartScene`** (nuestro): dibujos, niveles, eventos, órdenes y posiciones en
  coordenadas de dominio, versionados. Es lo que tiene valor y no depende del render.
- **`render-lwc`**: adaptador a Lightweight Charts v5 (primitivas, series custom,
  multi-panel). La interacción (selección, arrastre, atajos) es nuestra.
- Renderer propio **sólo** si el spike de M3 no cumple los criterios de salida
  (latencia de arrastre, fps con 200k barras, dibujos en el futuro, heatmap).
- Escala log/lineal, multi-panel, multi-layout sincronizado, atajos tipo TradingView,
  gestos táctiles (pinch zoom, arrastre con long-press, sin depender de hover).
- **Los indicadores y perfiles se calculan en `bender-engine`** y se sirven por `query`:
  la gráfica nunca calcula por su cuenta (una sola implementación para gráfica y alertas).

### 5.2 Temporalidades: de 1 minuto a 1 año

> Perfil liviano: mínimo 1m persistido; segundos sólo dentro de la ventana de trades o con
> el perfil al tick ([ADR-0014](decisiones/ADR-0014-perfiles-de-grabacion-y-resolucion-base.md)).

- Todo se agrega desde **trades**. Velas pre-calculadas (rollups) para las estándar:
  1s, 5s, 15s, 1m, 3m, 5m, 15m, **30m**, 1h, 2h, **4h**, 6h, 12h, 1D, 3D, 1W, 1M, 3M, 1Y.
  Las personalizadas (ej. 7m, 10h) se calculan al vuelo.
- Sub-minuto con historia profunda sólo donde hay trades (Binance desde 2017, BTCUSD
  desde 2011 en Bitstamp); en el resto, desde que arranca el grabador.
- Convenciones explícitas y configurables: día cripto cierra 00:00 UTC; semana empieza
  lunes; mes/año calendario.
- **Layout por defecto** pensado para vos: 4H + 30m sincronizados.

### 5.3 Dibujo, indicadores, overlays, alertas, replay

- Dibujo: horizontal, rayo, tendencia, rectángulo/zona, canal paralelo, Fibonacci,
  medidor, texto, posición larga/corta, **perfil de volumen anclado**, VWAP anclado.
  Atado al activo canónico; convertible en alerta, orden o "nivel mío" para el motor.
  **Versionado en el tiempo** (Pilar 2): "¿qué tenía dibujado el 10 de mayo?".
- Indicadores: clásicos + perfiles de volumen (sesión, rango, anclado, visible),
  VWAPs anclados, CVD, funding, OI, liquidaciones, basis spot-perp, divergencia de
  volumen spot vs perps. Más adelante: DSL propio que corre igual en gráfica, alertas y
  backtest, footprint, heatmap de libro.
- Overlays del motor: niveles, rangos/canales, eventos con fundamento al click.
- Alertas en servidor sobre el stream de trades → **push propio en la app + Telegram**.
- **Máquina del tiempo y replay** (Pilar 2): la app entera se posiciona en cualquier
  instante y le das play con cuenta paper.

---

## 6. Trading en la gráfica (spot)

- **Order lines**: línea horizontal → "comprar/vender acá" → monto → confirmar. Arrastrar
  modifica, click derecho cancela. Estados visibles (pendiente, parcial, llena).
- **Dónde vive la orden**, visible con ícono: limit **nativa** (default; sobrevive a cortes
  de tu casa) o **trigger de servidor** (con advertencia).
- **Escaleras** sobre zonas (rectángulo → N órdenes, distribución configurable).
- **Planes de acumulación**: capital asignado, ejecutado, promedio, pendiente.
- En el celular: mismas acciones, confirmación con **biometría** (firma del dispositivo,
  Pilar 3).

---

## 7. Ticket de futuros

1. Long / Short.
2. Entrada: market, limit o escalera.
3. **Stop loss obligatorio** por defecto.
4. **Riesgo en USD o %** → el tamaño se calcula solo.
5. Take profits múltiples (en precio o en R) + gestión opcional (BE en TP1, trailing).
6. Apalancamiento y modo de margen sugeridos a partir de lo anterior.

**Preview** antes de confirmar: tamaño, nocional, margen, **liquidación** (bloqueo si el
SL queda más allá), R:R, comisiones, funding estimado; posición dibujada en la gráfica y
ajustable arrastrando. **Plantillas** de setups.

Ejecución ([ADR-0002](decisiones/ADR-0002-garantias-ejecucion-futuros.md),
[doc 06](arquitectura/06-ejecucion-futuros.md)): **no se asume atomicidad en ningún
exchange**. Protección nativa con estrategia según capacidades verificadas (pre-armada >
adjunta > post-fill), saga por trade con estados `EXPOSED` / `UNCERTAIN` /
`EXTERNALLY_MODIFIED`, invariantes vigilados (INV-1…7), escalera de exposición con cierre
de emergencia, reconciliación contra el exchange y *make-before-break* para mover stops.
La ventana de exposición real de cada trade se mide y se muestra.

**Guardarraíles** autoimpuestos dentro de `execution` (pérdida diaria/semanal máxima,
cooldown tras pérdida, máximo de trades, riesgo máximo), desactivables con fricción y
registrados en el diario.

---

## 8. El motor de mercado

Corre como función pura y point-in-time ([Pilar 2](arquitectura/02-motor-historico-reproducible.md)),
no tiene permisos de operar ([Pilar 3](arquitectura/03-separacion-analisis-ejecucion.md))
y toda probabilidad que emite cumple el [Pilar 4](arquitectura/04-probabilidades-auditables.md).

```
[L1] Features      swings multi-TF, ATR, volumen relativo por venue, delta, VWAPs,
                   perfiles (spot / perps / agregado)
[L2] Niveles       perfiles de rango/canal, sesiones, anclados, S/R por swings,
                   VWAPs, redondos, Fibonacci, niveles tuyos — con procedencia
[L3] Eventos       máquinas de estado por nivel: toque → rechazo | aceptación |
                   ruptura → retest — definiciones versionadas
[L4] Confluencias  qué coincide en cada zona; pesos aprendidos, no inventados
[L5] Estadística   tasas base condicionadas + modelos calibrados (Pilar 4)
[L6] Narrador      plantillas deterministas (+ LLM opcional sin introducir números)
```

### 8.1 Rangos y canales, a tu manera

> **Estado: a definir con el usuario.** Primero se construye la herramienta genérica
> (perfil anclado que marcás vos, herramienta "marcar inicio de canal", canal paralelo
> dibujable) y se usa. Con eso se especifica el detector. Lo que sigue es la propuesta
> inicial, no una especificación cerrada. Los canales inclinados se tratan como un
> concepto aparte de los rangos de área.

Tu método: **desde el punto actual hacia atrás, hasta donde empieza el canal** en el que el
precio lateraliza. El ancla es el inicio del canal (en tu ejemplo, el máximo de 93000 del
10/05/2026 10:00), y el perfil va de ahí a ahora.

Cómo se automatiza:

1. **Candidatos a inicio**: los swings significativos hacia atrás desde ahora.
2. Para cada candidato `s`, se evalúa la ventana `[s, ahora]`:
   - **contención**: qué fracción de los cierres queda dentro de una banda estable;
   - **rotaciones**: cuántas veces el precio cruzó de un borde al otro;
   - **pendiente**: horizontal (rango) o inclinada y paralela (canal);
   - **estabilidad de volatilidad** dentro de la ventana;
   - qué había **antes** de `s`: un impulso/tendencia (un cambio de régimen detectado).
3. El inicio del canal es el `s` **más lejano** que todavía cumple, con el cambio de
   régimen justo antes.
4. **Multi-escala**: canales anidados (un canal de 30m dentro de uno de 4H dentro de uno
   diario). Tus temporalidades (4H y 30m) son las escalas por defecto en la gráfica.
5. El perfil anclado `[s, ahora]` da VAH/POC/VAL que **se actualizan en vivo**.
6. Cuando hay **aceptación fuera** del canal, se cierra y se archiva con sus valores
   finales: sigue siendo un nivel ("VAL del canal anterior").

**Aprendizaje de tu criterio**: una herramienta "marcar inicio de canal" en la UI. Marcás
inicios sobre el histórico (en la máquina del tiempo, sin ver el futuro), esas marcas
entrenan un modelo chico que elige el inicio entre los candidatos, y la métrica es la
distancia entre el inicio detectado y el tuyo. El detector por reglas es la línea base:
el modelo tiene que ganarle para reemplazarlo.

### 8.2 Otros detectores

- **S/R por swings**: clusters de pivots multi-TF con toques, edad, reacción, flips.
- **Sesiones**: perfiles diario, semanal, mensual; POCs vírgenes; HVN/LVN.
- **VWAPs anclados** desde eventos mayores.
- **Redondos, Fibonacci, niveles tuyos.**
- **Elliott** al final: conteos candidatos filtrados por reglas duras, puntuados, varios
  a la vez, ranking aprendido; se mide contra la tasa base como todo lo demás.

### 8.3 Eventos

Definiciones relativas a ATR, versionadas: **toque**, **rechazo** (cierre de vuelta +
mecha + sin aceptación), **aceptación** (N cierres del otro lado + volumen), **ruptura**,
**retest**, y **calidad del evento** (volumen relativo spot y perps, delta, velocidad de
llegada, número de toque).

### 8.4 Modelos

Separados como planteaste: **cripto / tradfi**, y por tipo: S/R, perfiles y canales,
confluencias, Elliott. Son modelos chicos (gradient boosting, redes 1D chicas) que
entrenan en la RTX 5050 sin problema. Etiquetas automáticas por resultado (triple barrera)
+ tus anotaciones. Validación walk-forward con purga y embargo.

Un LLM con LoRA no se usa para detectar patrones en precios: los LLM son malos con
secuencias numéricas largas y difíciles de calibrar. Se reserva para el lenguaje (§9).

### 8.5 Narrador

> El precio llegó a **80012** (VAL del canal 4H anclado en el máximo de **93000** del
> 10/05/2026 10:00 UTC; perfil spot agregado Binance+Bybit+BloFin). **Primer toque.
> Rechazo** en 4H con volumen relativo **0.6×** en spot y **0.9×** en perps.
> Históricamente, en 214 casos comparables (2017–2026, N efectivo 171), el **65%**
> [59–70%] tocó el **POC (84000)** antes de invalidar; tasa base 51%; validado
> walk-forward. Volumen bajo en el rechazo: **sin confirmación todavía**.

Cada número es clickeable y lleva a su procedencia o a su estimación auditable.

---

## 9. Diario de trading y LLM

### 9.1 Diario

Registro automático de cada trade (real y paper) con ejecución, R-múltiplo, ledger,
**snapshot del motor en el momento de la entrada** (vía máquina del tiempo, siempre
reconstruible), captura, tus campos (setup, razón, plan, emoción, nota) y flags
automáticos (sin SL, SL movido en contra, revenge, sobre-tamaño, guardarraíl desactivado,
entrada sin evento del motor). Estadísticas por setup/hora/activo/exchange, curva de
capital, drawdown, evolución mensual.

### 9.2 LLM: dos niveles, mismas herramientas

> Las herramientas las sirve **`bender-mcp`**, el servidor MCP de análisis, que también
> pueden usar Claude y Codex: [09-mcp-de-analisis.md](arquitectura/09-mcp-de-analisis.md),
> [ADR-0013](decisiones/ADR-0013-mcp-de-analisis.md).

| Dónde | Modelo | Para qué |
|---|---|---|
| **Celular** (S25) | **4B** cuantizado, en el dispositivo | charla diaria, consultas rápidas, notas de voz al diario |
| **Mini-PC** (RTX 5050) | 9B o **MoE 35B** | revisiones profundas: semanal, mensual, análisis de errores |

- **Mismo protocolo de herramientas** en ambos (estilo MCP), todas de **sólo lectura**
  (Pilar 3): `get_trades`, `get_trade`, `get_stats`, `get_equity_curve`,
  `get_account_state`, `get_engine_snapshot(símbolo, T)`, `get_events`, `get_levels`,
  `get_estimate(id)` (con sus casos), `search_journal`, `get_flags`, `get_rules`.
- **Aislamiento** ([ADR-0006](decisiones/ADR-0006-aislamiento-llm.md),
  [doc 08](arquitectura/08-aislamiento-llm.md)): cinco capas (código, herramientas,
  credenciales, red, presentación). En Android el LLM corre en un proceso aislado detrás de
  un broker de herramientas, y firmar una orden exige biometría por operación.
- El modelo del celular llama a las tools del servidor por Tailscale. Sin conexión, sólo
  puede charlar sobre un resumen local cacheado y lo dice.
- Runtime en el celular: llama.cpp (CPU/GPU Adreno) como base; evaluar el NPU Hexagon del
  Snapdragon 8 Elite (Qualcomm AI Engine / QNN) si mejora velocidad o batería.
- **LoRA del 4B**: se entrena en la mini-PC (QLoRA de 4B entra en 8 GB con secuencias
  moderadas), se fusiona, se cuantiza y se despliega al celular. Objetivo: usar bien las
  tools, en tu formato y con tu jerga. Dataset: conversaciones generadas sobre tus datos +
  tus correcciones. Evaluación: set fijo de preguntas con respuestas verificables contra la
  base de datos.
- Escribe sólo en "notas del asistente", separadas de tus registros. No recomienda
  entradas.

---

## 10. Stack

| Pieza | Elección |
|---|---|
| Esquema de datos | IDL único (Protobuf) → tipos Rust / TS |
| Daemon: marketdata, execution, api | **Rust** |
| Motor (features, niveles, eventos) | **Rust, una sola librería**; en vivo, replay, backtest y datasets |
| Research, ML | **Rust** (Polars, DuckDB, statrs; GBM/candle a evaluar en M12). Sin Python ([ADR-0012](decisiones/ADR-0012-sin-python.md)); única excepción posible: afinado LoRA, se decide en M11 |
| Clientes | TypeScript + `ChartScene` + Lightweight Charts v5; **Tauri 2** en PC y Android (mismo front; spike en M3) |
| Transporte | Connect-RPC/gRPC sobre Tailscale, streams reanudables por cursor ([ADR-0005](decisiones/ADR-0005-topologia-distribuida.md)) |
| LLM en celular | llama.cpp (+ evaluar QNN/NPU) en proceso aislado |
| LLM en estación | llama.cpp con la RTX 5050 |
| Datos | Parquet + DuckDB (crudo, rollups, research) · Postgres (estado) |
| Red | Tailscale; opcional VPS de salida con IP fija |
| Notificaciones | push propio de la app + Telegram |

---

## 11. Roadmap

El roadmap vive en [ROADMAP.md](ROADMAP.md): 15 milestones (M0–M14), cada uno con issues
en GitHub que tienen dependencias y criterios de aceptación verificables. Orden resumido:

M0 Fundaciones → **M1 Núcleo temporal** → M2 Datos (el grabador arranca apenas existen el
formato y el sobre) → M3 Gráfica (spikes en paralelo desde M0) → M4 Topología y seguridad
→ M5 Alertas → M6 Ejecución spot + paper → M7 Futuros → M8 Motor v1 → M9 Estadística
auditable → M10 Diario → M11 LLM → M12 ML → M13 Tradfi → M14 Elliott y extras.

## 12. Riesgos

- **Contaminación del futuro** → Pilar 2 y sus cuatro tests.
- **Buscar hasta encontrar (p-hacking)** → registro de hipótesis, N efectivo,
  encogimiento, calibración en vivo (Pilar 4).
- **Cambio de régimen** → features relativas, estadísticas por época.
- **Servidor en casa** (luz, internet, IP dinámica) → órdenes y SL/TP nativos, UPS,
  watchdog externo, IP fija o VPS de salida.
- **Órdenes huérfanas / posición sin SL** → brackets, rollback, reconciliación.
- **APIs que cambian o restringen** → adaptadores aislados, suite de conformidad.
- **Disco** → el grabador multi-venue crece rápido; retención y compresión planificadas.
- **Ventana de exposición en futuros** → invariantes, escalera de exposición, simulador
  con fallas y tests de caos ([doc 06](arquitectura/06-ejecucion-futuros.md)).
- **Porcentajes que son sólo geometría** → modelos base B2/B3 obligatorios (Pilar 4).
- **Usarlo como señal** → lenguaje del narrador, ninguna conexión motor→ejecución, y el
  diario midiendo si seguir o ignorar al motor te fue mejor.

---

## 13. Decisiones: cuáles ahora y cuáles después

Tiempo ilimitado no significa decidir todo hoy. Se decide **en el último momento
responsable**: cuando postergar cuesta más que decidir. Cada decisión diferida tiene un
disparador.

### Decididas
IP fija · desarrollo en la PC local · universo BTC/ETH · presupuesto de disco y retención
([ADR-0009](decisiones/ADR-0009-presupuesto-de-disco.md)) · archivado de irreemplazables y
gate de paso a la mini-PC ([ADR-0010](decisiones/ADR-0010-archivado-y-paso-a-produccion-del-grabador.md))
· rama `main` como tronco ([RAMAS.md](RAMAS.md)) · el primer entregable es el
[prototipo P0](PROTOTIPO.md).
· MCP de análisis: niveles de datos `mercado` para Claude/Codex y `completo` sólo para el LLM
local ([ADR-0013](decisiones/ADR-0013-mcp-de-analisis.md)) · sin Python ([ADR-0012](decisiones/ADR-0012-sin-python.md)).

### Diferidas, con su disparador
| Decisión | Se decide cuando… |
|---|---|
| Definiciones de canales, rangos de área y eventos | se use la herramienta genérica (M3) y empiece M8 (#98 marcado `necesita-refinamiento`) |
| Dónde está la GPU (PC o mini-PC) | empiece M9 o M11 (sólo cambia el perfil de despliegue) |
| Cuándo pasa el grabador a la mini-PC | se pase el gate de producción #140 (ADR-0010) |
| Shell móvil y canal de push | termine el spike #45 |
| Confirmar Lightweight Charts | termine el spike #44 |
| Compra de datos tradfi intradía | se cierre M9 (#133) |
| Exchanges secundarios para operar | terminen los spikes #79–#82 |
| VPS o disco extra (y encender el archivado de L2 crudo) | el monitoreo de disco (#43) proyecte < 90 días de espacio, o se quiera conservar el L2 crudo |
