# ADR-0014 — Perfiles de grabación, resolución base de 1 minuto y procedencia de los datos

- Estado: **aceptada**
- Fecha: 2026-10-28
- Reemplaza: la política de retención de [ADR-0009](ADR-0009-presupuesto-de-disco.md) y las
  clases de archivado de [ADR-0010](ADR-0010-archivado-y-paso-a-produccion-del-grabador.md)
  (el gate de la mini-PC de ADR-0010 sigue vigente)

## Contexto

Objetivo rector del usuario: **la mayor cantidad de herramientas buenas con el menor
tamaño posible**. Opera desde 15m en adelante y quiere al menos 2m y 5m. Las velas de
segundos y los ticks casi no se usan y son lo que más pesa. A la vez, la puerta tiene que
quedar abierta a guardar todo al tick si mañana hay un servidor con mucho disco.

También quiere que los datos de mercado no se redistribuyan: cada usuario los consigue de
la fuente oficial. Y dejar abierta la posibilidad de acuerdos de datos con exchanges.

## Decisión

### 1. Perfiles de grabación (mismo código, distinta configuración)

| Perfil | Persistente | Ventana rotativa (se borra al vencer) |
|---|---|---|
| **liviano** (default) | velas de 1m por instrumento; volumen por banda de precio por minuto (compra/venta); resumen de liquidez por banda y minuto; funding, mark/index, open interest, liquidaciones; huecos y recuperaciones | trades crudos (default 14 días); deltas de libro (default 2 días); latidos (con la misma ventana que los trades) |
| **al tick** (servidor propio grande) | todo lo anterior **más** trades y libro crudo para siempre | — |

- El núcleo temporal ya soporta velas de segundos y replay al evento: **no se quita**.
  El perfil decide qué se persiste.
- Cada ventana es configurable por clase de dato. Antes de borrar, el usuario puede
  activar el archivado comprimido (ADR-0010) si tiene dónde.

### 2. Resolución base de 1 minuto

- En el perfil liviano, la temporalidad mínima persistida y servida es **1m**. Desde 1m se
  derivan **exactamente** todas las demás (2m, 3m, 5m, 15m… 1Y), porque todas están
  alineadas a múltiplos de minuto. Las velas de 1m se arman desde trades, como hasta ahora.
- Dentro de la ventana de trades crudos, las temporalidades de segundos siguen
  disponibles. Fuera de ella, no.
- **Reproducibilidad**: dentro de la ventana de trades, al evento (los tests normativos y
  el test dorado vivo = replay siguen igual); fuera de ella, **al minuto**. Las preguntas
  estadísticas (M9) con objetivo e invalidación en el mismo minuto quedan `AMBIGUOUS`
  (Pilar 4 §1).

### 3. Resúmenes en lugar de crudo

- **Volumen por banda de precio por minuto**, separado en comprador/vendedor. Mantiene
  precisos los perfiles de volumen (VAH/POC/VAL), el delta, el CVD y un footprint liviano.
- **Liquidez por banda de precio por minuto**, para el heatmap: promedio ponderado en el
  tiempo y máximo de la liquidez en reposo, por lado, dentro de un rango alrededor del
  precio.
- El tamaño de banda es configurable por instrumento y **versionado**. Si cambia, los
  resúmenes viejos conservan su versión y siguen siendo comparables entre sí.

### 4. Historia: la descarga la hace el usuario, de la fuente oficial

- **Binance y Bybit**: la app guía al usuario a los archivos públicos oficiales. La
  descarga sale **de su dispositivo, directo del exchange**, nunca de un servidor nuestro.
  La app importa los archivos: velas de 1m como camino liviano, trades para reconstruir
  los resúmenes como camino preciso.
- **BloFin, BingX, MEXC** (y otros sin archivos públicos): no se ofrece descarga de
  historia. Existe lo que el grabador del propio usuario capta en vivo de la API pública y
  el relleno de huecos que esa API permita.
- El servidor de cada usuario **sólo sirve a su dueño**. El modo producto se decide más
  adelante.

### 5. Procedencia y términos de uso de cada dato

- Cada segmento registra su **fuente** (exchange, endpoint o archivo oficial), su
  **perfil** y sus **términos**: `uso_personal` (default) o `licenciado:<acuerdo>`.
- La capa que sirve datos (`bender-query`, `bender-mcp`) sólo entrega a terceros lo que
  tenga términos que lo permitan. Hoy nada los tiene: todo es `uso_personal`.
- Si en el futuro hay un acuerdo con un exchange para redistribuir sus datos, se carga el
  acuerdo y se etiquetan sus fuentes. **No hay que rehacer nada.** Exchanges candidatos:
  BloFin, BingX, MEXC, Bybit, Bitunix, entre otros.

### 6. Compresión

Antes de diseñar un formato propio se mide: Parquet con codificación nativa contra un
bloque propio (precios como enteros en ticks, diferencias entre valores consecutivos,
zstd), en tamaño y en velocidad de descompresión en el S25. El formato propio se adopta
sólo si gana claramente.

## Alternativas descartadas

- **Guardar todo al tick por defecto**: llena el disco por algo que el usuario casi no usa.
- **Velas de 1m sin resúmenes**: los perfiles de volumen quedarían aproximados, y son la
  herramienta central del usuario.
- **No guardar trades crudos en ningún momento**: se pierden la recuperación de huecos, la
  validación de velas y los tests de vivo = replay.
- **Servir datos de mercado a terceros desde nuestro servidor**: es redistribución, salvo
  un acuerdo explícito.

## Consecuencias

- El disco deja de ser una restricción en el perfil liviano. La alarma de disco de #43
  sigue existiendo.
- La gráfica, en el perfil liviano, muestra temporalidades de segundos sólo dentro de la
  ventana de trades.
- El heatmap tiene historia sólo desde que cada usuario graba: ningún exchange publica el
  libro histórico.
