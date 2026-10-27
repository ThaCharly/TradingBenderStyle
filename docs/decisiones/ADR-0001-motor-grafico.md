# ADR-0001 — Motor gráfico: Lightweight Charts v5 detrás de una capa de escena propia

- Estado: **aceptada** (revoca la decisión "renderer propio" del plan v2)
- Fecha: 2026-10-27
- Verificación pendiente: spike #M3-spike (criterios en §6)

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

El issue del spike en M3 implementa con A: velas de 1 s con 200k barras, un perfil de
volumen anclado, una línea de tendencia y un rectángulo arrastrables, una línea de orden
arrastrable, un dibujo anclado en el futuro, 4 gráficas sincronizadas, y mide los
criterios de §5 en PC y en el S25 (WebView de Tauri). El resultado se registra en esta ADR.
