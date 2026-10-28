# Spike #44: Lightweight Charts v5

Prototipo **descartable** que mide los criterios de salida de
[ADR-0001 §5](../../docs/decisiones/ADR-0001-motor-grafico.md). No es código de la app: la
app usa `apps/client/packages/chart-scene` y el adaptador `render-lwc`.

Qué tiene: velas de 1 s con 200.000 barras (sintéticas), escala log/lineal, 36 niveles,
10 líneas de orden arrastrables con etiqueta en el eje, una línea de tendencia y un
rectángulo arrastrables (capa de interacción propia), un rectángulo anclado en el futuro,
2 perfiles de volumen anclados (primitivas de panel), un heatmap 1.000 × 500 por
`ImageBitmap`/`OffscreenCanvas` y 4 gráficas (1s, 1m, 5m, 15m) sincronizadas por tiempo
(rango y crosshair). El botón **Medir** corre las mediciones y muestra el resultado.

```bash
pnpm install --ignore-workspace
```
```bash
pnpm dev
```

Medición automática en Chromium (resultado y capturas en `resultados/`):

```bash
pnpm build && NODE_PATH=$(npm root -g) node measure.mjs
```

`node build-artifact.mjs` arma `dist/artifact.html`, la versión que se publica para abrir
desde el teléfono.
