# ADR-0001 — Motor gráfico: Lightweight Charts v5 detrás de una capa de escena propia

- Estado: **aceptada** (revoca la decisión "renderer propio" del plan v2)
- Fecha: 2026-10-27
- Verificación pendiente: spike #44 (M3-01, criterios en §5–6)

## 1. Contexto

El plan v2 elegía un renderer propio "porque con tiempo infinito, controlar el renderer
permite que niveles, perfiles y órdenes sean ciudadanos de primera clase". Ese fundamento
era débil: el tiempo ilimitado no hace gratis el mantenimiento, y la decisión no comparaba
capacidades concretas. Esta ADR la reevalúa.

Necesidades reales de la gráfica:

1. Velas de 1 s a 1 año, escala log/lineal, multi-panel, varias gráficas sincronizadas.
2. Overlays del motor: niveles, cajas de canal, perfiles de volumen (horizontales, dentro
   del área de precio), marcadores de eventos clickeables.
3. Dibujo interactivo: crear, seleccionar, arrastrar y editar líneas, rectángulos, canales,
   Fibonacci y perfiles anclados; dibujos anclados en el **futuro** (a la derecha de la
   última vela).
4. Líneas de órdenes y posiciones **arrastrables** con estado.
5. Más adelante: heatmap de libro, footprint.
6. Fluidez en PC y en un WebView de Android, con gestos táctiles.

## 2. Opciones

| | **A. Lightweight Charts v5 + plugins** | **B. Renderer propio** | **C. TradingView Advanced Charts** | **D. KLineChart** | **E. SciChart.js** |
|---|---|---|---|---|---|
| Licencia | Apache-2.0 | propia | gratuita con aprobación, orientada a sitios públicos de empresas; no pensada para uso personal; sin código fuente | Apache-2.0 | comercial, paga |
| Velas, escalas, ejes, crosshair, zoom, inercia, HiDPI, táctil | **resuelto upstream** | todo a construir | resuelto | resuelto | resuelto |
| Multi-panel | sí (v5) | a construir | sí | sí | sí |
| Overlays custom | primitivas de serie y de panel (dibujo en canvas con z-order, vistas propias en los ejes) | total | API de shapes cerrada | overlays propios extensibles | total |
| Series custom (footprint, etc.) | sí (custom series) | total | limitado | sí (indicadores custom) | total |
| Herramientas de dibujo | **no incluidas**; hay ejemplos oficiales de plugins; la interacción (hit-test, arrastre) es nuestra | todo a construir | incluidas y completas | **incluidas** | parcial |
| Dibujos en el futuro | requiere datos "whitespace" o coordenadas lógicas | total | sí | sí | sí |
| Rendimiento | Canvas 2D; bien hasta cientos de miles de barras con carga por ventanas; heatmap vía `ImageBitmap` dentro de una primitiva | WebGL posible | bueno | Canvas 2D | **WebGL**, muy alto |
| Mantenimiento | upstream mantiene el núcleo; nosotros mantenemos plugins y la capa de interacción; riesgo: cambios de API entre mayores (v3→v4→v5 los hubo) | **todo nuestro**, decenas de miles de líneas: texto, ticks de ejes, gestos, HiDPI, accesibilidad | dependencia cerrada | comunidad más chica | proveedor |

Límites conocidos de A (a confirmar en el spike):
- El tiempo de las series es en **segundos**: no hay velas sub-segundo. No las
  necesitamos (mínimo 1 s); el replay trade a trade se muestra en velas de 1 s.
- Una escala de tiempo por gráfica, **basada en índice de barras**: los huecos se comprimen
  (bueno para tradfi con fines de semana). Overlays de otra temporalidad se calculan del
  lado nuestro y se dibujan por coordenadas.
- No trae arrastre de dibujos: hay que interceptar eventos del puntero, convertir con
  `coordinateToPrice` / `coordinateToLogical` y desactivar temporalmente el scroll del
  chart durante el arrastre.
- Sólo Canvas 2D. Para heatmaps grandes: dibujar en un canvas fuera de pantalla (o
  WebGL propio) y componerlo en la primitiva.

## 3. Decisión

**A, detrás de una capa de escena propia (`ChartScene`).**

- `ChartScene` es nuestro modelo de dominio: dibujos, niveles, eventos, órdenes y
  posiciones en **coordenadas de dominio** (tiempo UTC, precio, activo canónico),
  versionados (Pilar 2). No sabe nada de Lightweight Charts.
- Un **adaptador de render** (`render-lwc`) traduce `ChartScene` a series y primitivas de
  Lightweight Charts y traduce los gestos del usuario a comandos sobre la escena.
- La capa de interacción (selección, hit-test, arrastre, atajos) es nuestra y vive sobre
  la escena, no dentro de los plugins.

Fundamento:
- Lo que da valor al proyecto (escena de dominio, interacción, overlays del motor) lo
  construimos igual en A y en B. Lo que B agrega sin que lo necesitemos es el núcleo de
  render, que es justamente lo más caro de mantener y lo que A ya resuelve bien
  (ejes, HiDPI, gestos, inercia).
- C se descarta por licencia y por ser una caja cerrada. E por costo, sin una necesidad
  que lo justifique. D es la **alternativa de respaldo** si la capa de interacción
  sobre A resulta inviable, porque trae dibujo incluido.

## 4. Consecuencias

- Un costo inicial menor y un mantenimiento acotado a plugins y adaptador.
- La dependencia de un upstream: se fija una versión mayor y se actualiza a propósito, con
  los tests visuales del adaptador.
- Como la escena es independiente del render, **cambiar de render es reemplazar el
  adaptador**, no la aplicación.

## 5. Criterios de salida (cuándo pasar a renderer propio)

Si cualquiera de estos falla en el spike o después, se abre una ADR nueva para B (o D):
- no se logra arrastre de dibujos y órdenes con latencia ≤ 16 ms en PC y ≤ 33 ms en el S25;
- con 200.000 barras cargadas y 50 primitivas (niveles + 2 perfiles + 10 líneas de orden),
  el scroll y el zoom bajan de 60 fps en PC o de 30 fps en el S25;
- no se pueden anclar dibujos en el futuro de forma estable al cambiar de temporalidad;
- el heatmap de libro (1.000 × 500 celdas) no se dibuja en ≤ 16 ms por cuadro en PC.

## 6. Spike de validación

El issue del spike (#44) implementa con A: velas de 1 s con 200k barras, un perfil de
volumen anclado, una línea de tendencia y un rectángulo arrastrables, una línea de orden
arrastrable, un dibujo anclado en el futuro, 4 gráficas sincronizadas, y mide los
criterios de §5 en PC y en el S25 (WebView de Tauri). El resultado se registra en esta ADR.

## 7. Resultado del spike (#44)

Prototipo en [`spikes/lwc/`](../../spikes/lwc/README.md): Lightweight Charts 5.2.1, velas
de 1 s con 200.000 barras, 36 niveles + 10 líneas de orden + línea de tendencia +
rectángulo + rectángulo anclado en el futuro + 2 perfiles de volumen (50+ primitivas),
heatmap 1.000 × 500 y 4 gráficas (1s, 1m, 5m, 15m) sincronizadas por tiempo. El botón
**Medir** corre todas las mediciones en el dispositivo.

Cómo se mide:
- **Arrastre**: desde el `timeStamp` del evento de puntero hasta el primer cuadro en el que
  la primitiva ya pintó el estado nuevo (incluye la espera del vsync). Automático: 180
  movimientos sintéticos en momentos al azar entre cuadros; manual: los arrastres hechos
  con el mouse o el dedo antes de medir.
- **Scroll / zoom**: intervalo entre cuadros mientras se mueve el rango visible en cada
  cuadro (2.000 velas visibles; zoom de 300 a 30.000), con las 4 gráficas sincronizadas.
  La tasa está topeada por la pantalla, así que se aprueba con el 95 % del objetivo.
- **Heatmap**: cuadro completo con una columna nueva del libro por cuadro (búfer circular
  en un `OffscreenCanvas`, dos `drawImage` en una primitiva).
- **Futuro**: posición del rectángulo anclado a +3 min de la última vela en 1m, 5m, 15m y
  1s, contra la esperada por el espaciado de barras.

| Criterio (§5) | Chromium headless, 4 núcleos, sin GPU | S25 (WebView de la app de Claude, Android 16) | PC del dueño | Umbral |
|---|---|---|---|---|
| Arrastre, automático | p95 14,8 ms | p95 12,3 ms | pendiente | ≤ 16 ms PC, ≤ 33 ms S25 |
| Arrastre, con el dedo | — | p95 22,2 ms (21 muestras) | pendiente | ≤ 33 ms |
| Scroll, 200k velas + 50 primitivas + 4 gráficas | 60 fps (p95 16,7 ms) | 80 fps (p95 12,6 ms; tope de la pantalla) | pendiente | ≥ 60 PC, ≥ 30 S25 |
| Zoom | 60 fps | 80 fps | pendiente | ídem |
| Heatmap 1.000 × 500 por cuadro | p95 16,7 ms (columna 2,2 ms; `drawImage` 0,2 ms) | p95 12,6 ms (columna 0,7 ms; `drawImage` 0,1 ms) | pendiente | ≤ 16 ms PC |
| Dibujo en el futuro estable al cambiar de TF | error 0 px | error 0 px | pendiente | estable |

**Decisión: ADR-0001 confirmada** (pendiente sólo la medición en la PC del dueño, que no
cambia la decisión salvo un resultado muy distinto del headless, que corre sin GPU).

Limitaciones encontradas:
1. **`logicalToCoordinate` sólo acepta índices enteros**: con uno fraccionario devuelve 0.
   Todo dibujo cuyo tiempo no cae justo en una vela (por ejemplo, una línea trazada en 1 s
   vista en 5m) se dibujaba en el borde izquierdo. Resuelto: el adaptador interpola entre
   los dos enteros vecinos (`logicalX` en el spike). Va al adaptador `render-lwc`.
2. **Tiempo ↔ índice lógico es nuestro**: la escala es por índice de barras. El spike
   convierte tiempo de dominio → índice (interpolando dentro de los datos y extrapolando a
   la derecha con la temporalidad); así los dibujos anclados en el futuro quedan en el
   mismo instante en cualquier TF. Resuelto en la capa de escena.
3. **Sincronizar gráficas de distinta temporalidad** se hace por tiempo, no por índice:
   cada gráfica convierte el rango visible de la que se movió. Funciona; hay que evitar
   el rebote de eventos (bandera mientras se sincroniza).
4. **Arrastre**: no viene en la librería. La capa propia (hit-test en `pointerdown` en fase
   de captura, `handleScroll`/`handleScale` apagados durante el gesto) funcionó con mouse y
   con el dedo sin que la gráfica se desplace.
5. **Heatmap**: recomponer las 500.000 celdas tarda 110–170 ms (headless y S25), así que se
   hace sólo cuando cambia el rango de precios; cada actualización del libro escribe una
   columna (≤ 2,2 ms).
6. Etiquetas de órdenes en el eje de precio: con `priceAxisViews` de la primitiva; se
   superponen si hay muchas juntas (a resolver en la UI, no es un límite de la librería).
