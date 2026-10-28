// Mediciones automáticas de los criterios de ADR-0001 §5.

import type { Logical } from "lightweight-charts";

import type { ChartBox } from "./main";
import type { HLine, Heatmap, Rect, TrendLine, VolumeProfile } from "./primitives";
import { timeToLogical } from "./data";

export interface Scene {
  levels: HLine[];
  orders: HLine[];
  trend: TrendLine;
  rect: Rect;
  future: Rect;
  profiles: VolumeProfile[];
  heatmap: Heatmap;
}

export interface Ctx {
  boxes: ChartBox[];
  main: ChartBox;
  scene: Scene;
  dragLatencies: number[];
  setTimeframe: (box: ChartBox, tf: number) => void;
  onDragMove: (e: PointerEvent) => void;
}

export interface Stats {
  n: number;
  p50: number;
  p95: number;
  max: number;
}

export interface Results {
  device: { userAgent: string; dpr: number; cores: number; width: number; height: number; mobile: boolean };
  drag: Stats;
  scroll: { fps: number; frame: Stats };
  zoom: { fps: number; frame: Stats };
  heatmap: { fps: number; frame: Stats; compose: Stats; draw: Stats; fullComposeMs: number };
  future: { ok: boolean; maxErrorPx: number; detail: string[] };
  manualDrag: Stats | null;
  pass: Record<string, boolean>;
  text: string;
}

function stats(xs: number[]): Stats {
  if (xs.length === 0) return { n: 0, p50: NaN, p95: NaN, max: NaN };
  const s = [...xs].sort((a, b) => a - b);
  const q = (p: number): number => s[Math.min(s.length - 1, Math.floor(p * s.length))];
  return { n: s.length, p50: q(0.5), p95: q(0.95), max: s[s.length - 1] };
}

const frame = (): Promise<number> => new Promise((r) => requestAnimationFrame(r));

/** Corre `step` una vez por cuadro durante `ms` y mide los intervalos entre cuadros. */
async function frames(ms: number, step: (i: number) => void): Promise<{ fps: number; frame: Stats }> {
  await frame();
  const intervals: number[] = [];
  const start = await frame();
  let prev = start;
  let i = 0;
  while (prev - start < ms) {
    step(i++);
    const now = await frame();
    intervals.push(now - prev);
    prev = now;
  }
  return { fps: (intervals.length * 1000) / (prev - start), frame: stats(intervals) };
}

function pointer(type: string, el: HTMLElement, x: number, y: number): PointerEvent {
  const r = el.getBoundingClientRect();
  return new PointerEvent(type, {
    bubbles: true,
    cancelable: true,
    pointerId: 1,
    pointerType: "mouse",
    isPrimary: true,
    clientX: r.left + x,
    clientY: r.top + y,
    buttons: type === "pointerup" ? 0 : 1,
  });
}

const sleep = (ms: number): Promise<void> => new Promise((r) => setTimeout(r, ms));

/** Arrastra de (x, y) moviéndose `dx, dy` por paso durante `n` pasos. Los eventos llegan en
 * momentos al azar entre cuadros, como los de un mouse o un dedo. */
async function syntheticDrag(el: HTMLElement, x: number, y: number, dx: number, dy: number, n: number): Promise<void> {
  el.dispatchEvent(pointer("pointerdown", el, x, y));
  for (let i = 1; i <= n; i++) {
    await sleep(4 + Math.random() * 14);
    el.dispatchEvent(pointer("pointermove", el, x + dx * i, y + dy * i));
  }
  await sleep(40);
  el.dispatchEvent(pointer("pointerup", el, x + dx * n, y + dy * n));
  await frame();
  await frame();
}

export async function runAll(ctx: Ctx): Promise<Results> {
  const { main, scene } = ctx;
  const ts = main.chart.timeScale();
  const mobile = /Android|iPhone|iPad/i.test(navigator.userAgent);
  const manualDrag = ctx.dragLatencies.length > 0 ? stats(ctx.dragLatencies) : null;
  const n = main.frame.bars.length;
  const view = (from: number, to: number): void => ts.setVisibleLogicalRange({ from: from as Logical, to: to as Logical });

  // 1. Arrastre: orden, línea de tendencia y rectángulo (eventos sintéticos).
  view(n - 1800, n + 900);
  await frame();
  ctx.dragLatencies.length = 0;
  const p = main.projector;
  const order = scene.orders[0];
  const oy = p.y(order.price) ?? 100;
  await syntheticDrag(main.el, 80, oy, 0, -1, 60);
  const tx = ((p.x(scene.trend.a.time) ?? 0) + (p.x(scene.trend.b.time) ?? 0)) / 2;
  const ty = ((p.y(scene.trend.a.price) ?? 0) + (p.y(scene.trend.b.price) ?? 0)) / 2;
  await syntheticDrag(main.el, tx, ty, 1, 0.5, 60);
  const box = scene.rect.box();
  if (box) await syntheticDrag(main.el, box[0] + box[2] / 2, box[1] + box[3] / 2, -1, 0.3, 60);
  const drag = stats(ctx.dragLatencies);
  ctx.dragLatencies.length = 0;

  // 2. Scroll con ~2.000 velas visibles, 50 primitivas y 4 gráficas sincronizadas.
  const width = 2000;
  const scroll = await frames(4000, (i) => {
    const from = n - width - 20_000 + i * 40;
    view(from, from + width);
  });

  // 3. Zoom: de 300 a 30.000 velas visibles y vuelta.
  const zoom = await frames(4000, (i) => {
    const phase = (i % 120) / 120;
    const w = 300 + (30_000 - 300) * (phase < 0.5 ? phase * 2 : 2 - phase * 2);
    view(n - w, n + 50);
  });

  // 4. Heatmap 1.000 × 500 recompuesto en cada cuadro.
  view(n - 1200, n + 100);
  scene.heatmap.enabled = true;
  const fullCompose = scene.heatmap.composeAll();
  const compose: number[] = [];
  const draw: number[] = [];
  const heat = await frames(4000, () => {
    scene.heatmap.compose();
    compose.push(scene.heatmap.composeMs);
    if (scene.heatmap.drawMs > 0) draw.push(scene.heatmap.drawMs);
    scene.heatmap.update();
  });
  scene.heatmap.enabled = false;
  scene.heatmap.update();

  // 5. Dibujo anclado en el futuro: misma posición temporal en cada TF.
  const detail: string[] = [];
  let maxErrorPx = 0;
  let ok = true;
  const anchor = scene.future.a.time;
  for (const tf of [60, 300, 900, 1]) {
    ctx.setTimeframe(main, tf);
    const bars = main.frame.bars;
    const m = bars.length;
    view(m - 120, m + 40);
    await frame();
    await frame();
    const x = p.x(anchor);
    const last = bars[m - 1].time;
    const expectedLogical = m - 1 + (anchor - last) / tf;
    const spacing = (ts.logicalToCoordinate(m as Logical) ?? 0) - (ts.logicalToCoordinate((m - 1) as Logical) ?? 0);
    const lastX = ts.logicalToCoordinate((m - 1) as Logical) ?? 0;
    const expected = lastX + (expectedLogical - (m - 1)) * spacing;
    const err = x === null ? Infinity : Math.abs(x - expected);
    const sameAnchor = Math.abs(timeToLogical(bars, tf, anchor) - expectedLogical) < 1e-9;
    maxErrorPx = Math.max(maxErrorPx, err);
    if (!(err < 0.5) || !sameAnchor || scene.future.a.time !== anchor) ok = false;
    detail.push(`TF ${tf}s: x=${x?.toFixed(1)} esperado=${expected.toFixed(1)} (error ${err.toFixed(2)} px)`);
  }
  view(n - 1800, n + 900);

  const dragLimit = mobile ? 33 : 16;
  const fpsLimit = mobile ? 30 : 60;
  // La tasa de cuadros está topeada por la pantalla (vsync): se aprueba con el 95 % del objetivo.
  const pass = {
    arrastre: drag.p95 <= dragLimit,
    scroll: scroll.fps >= fpsLimit * 0.95,
    zoom: zoom.fps >= fpsLimit * 0.95,
    heatmap: heat.frame.p95 <= (mobile ? 33 : 16.7) * 1.05,
    futuro: ok,
  };
  const f = (x: number): string => (Number.isFinite(x) ? x.toFixed(1) : "-");
  const mark = (b: boolean): string => (b ? "✓" : "✗");
  const lines = [
    `Spike #44 — ${mobile ? "teléfono" : "PC"} · dpr ${devicePixelRatio} · ${navigator.hardwareConcurrency} núcleos · ${innerWidth}×${innerHeight}`,
    navigator.userAgent,
    `${mark(pass.arrastre)} Arrastre (orden, tendencia, rectángulo; ${drag.n} muestras): p50 ${f(drag.p50)} ms, p95 ${f(drag.p95)} ms, máx ${f(drag.max)} ms (umbral ${dragLimit} ms)`,
    `${mark(pass.scroll)} Scroll (2.000 velas visibles de 200.000, 50 primitivas, 4 gráficas): ${f(scroll.fps)} fps, cuadro p95 ${f(scroll.frame.p95)} ms (umbral ${fpsLimit} fps)`,
    `${mark(pass.zoom)} Zoom (300 ↔ 30.000 velas visibles): ${f(zoom.fps)} fps, cuadro p95 ${f(zoom.frame.p95)} ms`,
    `${mark(pass.heatmap)} Heatmap 1.000×500 con una columna nueva por cuadro: ${f(heat.fps)} fps, cuadro p95 ${f(heat.frame.p95)} ms; columna p95 ${f(stats(compose).p95)} ms; drawImage p95 ${f(stats(draw).p95)} ms; recomposición completa ${f(fullCompose)} ms`,
    `${mark(pass.futuro)} Dibujo en el futuro estable al cambiar de TF: error máx ${f(maxErrorPx)} px`,
    ...detail.map((d) => `    ${d}`),
    manualDrag
      ? `   Arrastre manual previo (${manualDrag.n} muestras): p50 ${f(manualDrag.p50)} ms, p95 ${f(manualDrag.p95)} ms`
      : `   (Sin arrastre manual: si arrastrás una orden o la línea antes de medir, también se reporta.)`,
  ];
  return {
    device: { userAgent: navigator.userAgent, dpr: devicePixelRatio, cores: navigator.hardwareConcurrency, width: innerWidth, height: innerHeight, mobile },
    drag,
    scroll,
    zoom,
    heatmap: { ...heat, compose: stats(compose), draw: stats(draw), fullComposeMs: fullCompose },
    future: { ok, maxErrorPx, detail },
    manualDrag,
    pass,
    text: lines.join("\n"),
  };
}
