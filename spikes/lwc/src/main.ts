// Spike #44: Lightweight Charts v5 contra los criterios de ADR-0001 §5.

import {
  CandlestickSeries,
  type IChartApi,
  type ISeriesApi,
  type Logical,
  type MouseEventParams,
  PriceScaleMode,
  type Time,
  type UTCTimestamp,
  createChart,
} from "lightweight-charts";

import {
  type Bar,
  aggregate,
  logicalToTime,
  syntheticBars,
  timeToLogical as timeToLogicalFn,
  toSeries,
} from "./data";
import { type ChartFrame, HLine, Heatmap, type Point, Projector, Rect, TrendLine, VolumeProfile } from "./primitives";
import { type Results, runAll } from "./bench";

const N_BARS = 200_000;
const END = Math.floor(Date.UTC(2026, 9, 27, 12, 0, 0) / 1000);
const BASE: Bar[] = syntheticBars(N_BARS, END);
const TFS = [1, 60, 300, 900];
const TF_NAMES: Record<number, string> = { 1: "1s", 60: "1m", 300: "5m", 900: "15m" };

// ---------------------------------------------------------------------------- gráficas

export interface ChartBox {
  chart: IChartApi;
  series: ISeriesApi<"Candlestick">;
  frame: ChartFrame;
  projector: Projector;
  el: HTMLElement;
}

function makeChart(el: HTMLElement, tf: number): ChartBox {
  const chart = createChart(el, {
    autoSize: true,
    layout: { background: { color: "#131722" }, textColor: "#d1d4dc", attributionLogo: false },
    grid: { vertLines: { color: "#1f2430" }, horzLines: { color: "#1f2430" } },
    timeScale: { timeVisible: true, secondsVisible: tf < 60, rightOffset: 20 },
    crosshair: { mode: 0 },
  });
  const series = chart.addSeries(CandlestickSeries, {
    upColor: "#26a69a",
    downColor: "#ef5350",
    borderVisible: false,
    wickUpColor: "#26a69a",
    wickDownColor: "#ef5350",
  });
  const bars = aggregate(BASE, tf);
  series.setData(toSeries(bars));
  const frame = { bars, tf };
  const projector = new Projector(frame);
  projector.chart = chart;
  projector.series = series;
  return { chart, series, frame, projector, el };
}

const grid = document.getElementById("charts")!;
const boxes: ChartBox[] = TFS.map((tf, i) => {
  const cell = document.createElement("div");
  cell.className = "cell";
  const tag = document.createElement("span");
  tag.className = "tag";
  tag.textContent = TF_NAMES[tf] + (i === 0 ? " · 200.000 velas · 50 primitivas" : "");
  cell.appendChild(tag);
  grid.appendChild(cell);
  return makeChart(cell, tf);
});
const main = boxes[0];
document.getElementById("status")!.textContent = `${N_BARS.toLocaleString("es")} velas de 1 s cargadas`;

// ------------------------------------------------------------------- sincronización

let syncing = false;
function syncRange(from: ChartBox): void {
  if (syncing) return;
  const range = from.chart.timeScale().getVisibleLogicalRange();
  if (!range) return;
  const t0 = logicalToTime(from.frame.bars, from.frame.tf, range.from);
  const t1 = logicalToTime(from.frame.bars, from.frame.tf, range.to);
  syncing = true;
  for (const b of boxes) {
    if (b === from) continue;
    const l0 = timeToLogicalOf(b, t0);
    const l1 = timeToLogicalOf(b, t1);
    b.chart.timeScale().setVisibleLogicalRange({ from: l0 as Logical, to: l1 as Logical });
  }
  syncing = false;
}

function timeToLogicalOf(b: ChartBox, t: number): number {
  return b.projector.frame.bars.length > 0 ? timeToLogicalFn(b.frame.bars, b.frame.tf, t) : 0;
}

function syncCrosshair(from: ChartBox, param: MouseEventParams<Time>): void {
  if (syncing) return;
  syncing = true;
  for (const b of boxes) {
    if (b === from) continue;
    if (param.time === undefined || !param.point) {
      b.chart.clearCrosshairPosition();
      continue;
    }
    const price = from.series.coordinateToPrice(param.point.y);
    const t = Math.floor((param.time as number) / b.frame.tf) * b.frame.tf;
    if (price !== null) b.chart.setCrosshairPosition(price, t as UTCTimestamp, b.series);
  }
  syncing = false;
}

for (const b of boxes) {
  b.chart.timeScale().subscribeVisibleLogicalRangeChange(() => syncRange(b));
  b.chart.subscribeCrosshairMove((p) => syncCrosshair(b, p));
}

// ------------------------------------------------------------------ primitivas (50)

const last = BASE[BASE.length - 1];
const p = main.projector;
const at = (secondsBack: number): number => last.time - secondsBack;
const px = last.close;

const levels: HLine[] = [];
for (let i = 0; i < 36; i++) {
  levels.push(new HLine(p, px + (i - 18) * 25, i % 3 === 0 ? "#787b86" : "#4a4e5a", null, i % 2 === 0));
}
const orders: HLine[] = [];
for (let i = 0; i < 10; i++) {
  const buy = i < 5;
  orders.push(new HLine(p, px + (buy ? -(30 + i * 22) : 30 + (i - 5) * 22), buy ? "#26a69a" : "#ef5350", buy ? "BUY" : "SELL"));
}
const trend = new TrendLine(p, { time: at(900), price: px - 60 }, { time: at(120), price: px + 40 });
const rect = new Rect(p, { time: at(600), price: px + 80 }, { time: at(240), price: px + 20 });
// Anclado en el futuro: de +3 min a +12 min después de la última vela.
const future = new Rect(
  p,
  { time: last.time + 180, price: px + 60 },
  { time: last.time + 720, price: px - 60 },
  "rgba(171,71,188,0.15)",
  "#ab47bc",
);
for (const prim of [...levels, ...orders, trend, rect, future]) main.series.attachPrimitive(prim);
const profiles = [new VolumeProfile(p, at(1800), at(900)), new VolumeProfile(p, at(800), at(0) + 1)];
for (const prof of profiles) main.chart.panes()[0].attachPrimitive(prof);
const heatmap = new Heatmap(p, 1000, 500, px - 150, px + 150);
main.series.attachPrimitive(heatmap);
heatmap.composeAll();

export const scene = { levels, orders, trend, rect, future, profiles, heatmap };

// Vista inicial: la última media hora y 15 minutos de futuro a la derecha.
main.chart.timeScale().setVisibleLogicalRange({
  from: (BASE.length - 1800) as Logical,
  to: (BASE.length + 900) as Logical,
});

// ----------------------------------------------------- interacción: arrastre propio

type Drag =
  | { kind: "order"; line: HLine }
  | { kind: "trend"; part: "a" | "b" | "body"; start: Point; a: Point; b: Point }
  | { kind: "rect"; target: Rect; part: "b" | "body"; start: Point; a: Point; b: Point };

let drag: Drag | null = null;
/** Latencias medidas en arrastres (evento de puntero → cuadro pintado), en ms. */
export const dragLatencies: number[] = [];

function domainAt(x: number, y: number): Point | null {
  const logical = main.chart.timeScale().coordinateToLogical(x);
  const price = main.series.coordinateToPrice(y);
  if (logical === null || price === null) return null;
  return { time: logicalToTime(main.frame.bars, main.frame.tf, logical), price };
}

function localXY(e: PointerEvent): [number, number] {
  const r = main.el.getBoundingClientRect();
  return [e.clientX - r.left, e.clientY - r.top];
}

function setChartInteraction(on: boolean): void {
  main.chart.applyOptions({ handleScroll: on, handleScale: on });
}

main.el.addEventListener(
  "pointerdown",
  (e) => {
    const [x, y] = localXY(e);
    const start = domainAt(x, y);
    if (!start) return;
    const order = orders.find((o) => o.hit(x, y));
    const tp = trend.hit(x, y);
    const rp = rect.hit(x, y) ?? null;
    const fp = rp ? null : future.hit(x, y);
    if (order) drag = { kind: "order", line: order };
    else if (tp) drag = { kind: "trend", part: tp, start, a: { ...trend.a }, b: { ...trend.b } };
    else if (rp) drag = { kind: "rect", target: rect, part: rp, start, a: { ...rect.a }, b: { ...rect.b } };
    else if (fp) drag = { kind: "rect", target: future, part: fp, start, a: { ...future.a }, b: { ...future.b } };
    if (!drag) return;
    setChartInteraction(false);
    main.el.setPointerCapture(e.pointerId);
    e.stopPropagation();
  },
  { capture: true },
);

export function onDragMove(e: PointerEvent): void {
  if (!drag) return;
  const [x, y] = localXY(e);
  const cur = domainAt(x, y);
  if (!cur) return;
  if (drag.kind === "order") {
    drag.line.price = cur.price;
    drag.line.update();
  } else {
    const dt = cur.time - drag.start.time;
    const dp = cur.price - drag.start.price;
    const target = drag.kind === "trend" ? trend : drag.target;
    if (drag.part === "a" && drag.kind === "trend") trend.a = cur;
    else if (drag.part === "b") target.b = cur;
    else {
      target.a = { time: drag.a.time + dt, price: drag.a.price + dp };
      target.b = { time: drag.b.time + dt, price: drag.b.price + dp };
    }
    target.update();
  }
  // Latencia: desde el evento hasta el primer cuadro en el que la primitiva ya pintó el
  // estado nuevo (se mide en un rAF; si en ese cuadro todavía no pintó, se espera otro).
  const target = drag.kind === "order" ? drag.line : drag.kind === "trend" ? trend : drag.target;
  const version = target.version;
  const t0 = e.timeStamp;
  const check = (): void => {
    if (target.painted >= version) dragLatencies.push(performance.now() - t0);
    else requestAnimationFrame(check);
  };
  requestAnimationFrame(check);
}

main.el.addEventListener("pointermove", onDragMove, { capture: true });
const endDrag = (): void => {
  if (!drag) return;
  drag = null;
  setChartInteraction(true);
};
main.el.addEventListener("pointerup", endDrag, { capture: true });
main.el.addEventListener("pointercancel", endDrag, { capture: true });

// --------------------------------------------------------------- controles y medición

export function setTimeframe(box: ChartBox, tf: number): void {
  const range = box.chart.timeScale().getVisibleLogicalRange();
  const t0 = range ? logicalToTime(box.frame.bars, box.frame.tf, range.from) : 0;
  const t1 = range ? logicalToTime(box.frame.bars, box.frame.tf, range.to) : 0;
  box.frame.bars = aggregate(BASE, tf);
  box.frame.tf = tf;
  box.series.setData(toSeries(box.frame.bars));
  box.chart.applyOptions({ timeScale: { secondsVisible: tf < 60 } });
  if (range) {
    box.chart.timeScale().setVisibleLogicalRange({
      from: timeToLogicalFn(box.frame.bars, tf, t0) as Logical,
      to: timeToLogicalFn(box.frame.bars, tf, t1) as Logical,
    });
  }
  for (const prof of profiles) prof.compute();
}

let log = false;
document.getElementById("log")!.addEventListener("click", (ev) => {
  log = !log;
  for (const b of boxes) b.chart.priceScale("right").applyOptions({ mode: log ? PriceScaleMode.Logarithmic : PriceScaleMode.Normal });
  (ev.target as HTMLElement).textContent = log ? "Escala: log" : "Escala: lineal";
});
document.getElementById("heat")!.addEventListener("click", (ev) => {
  heatmap.enabled = !heatmap.enabled;
  heatmap.update();
  (ev.target as HTMLElement).textContent = heatmap.enabled ? "Heatmap: sí" : "Heatmap: no";
});

const out = document.getElementById("results")!;
const runBtn = document.getElementById("run") as HTMLButtonElement;
runBtn.addEventListener("click", async () => {
  runBtn.disabled = true;
  out.textContent = "Midiendo… (unos 20 s, no toques la pantalla)";
  try {
    const r = await runAll({ boxes, main, scene, dragLatencies, setTimeframe, onDragMove });
    show(r);
  } catch (e) {
    out.textContent = `Error: ${e}`;
  } finally {
    runBtn.disabled = false;
  }
});

function show(r: Results): void {
  out.textContent = r.text;
  (window as unknown as { benchResults: Results }).benchResults = r;
  const copy = document.getElementById("copy") as HTMLButtonElement;
  copy.hidden = false;
  copy.onclick = () => {
    navigator.clipboard
      .writeText(r.text)
      .then(() => {
        copy.textContent = "Copiado ✓";
      })
      .catch(() => {
        // Sin portapapeles: se selecciona el texto para copiarlo a mano.
        const range = document.createRange();
        range.selectNodeContents(out);
        const sel = getSelection();
        sel?.removeAllRanges();
        sel?.addRange(range);
        copy.textContent = "Seleccionado: copialo con el menú";
      });
  };
}

(window as unknown as { spike: unknown }).spike = { boxes, scene, runAll: () => runAll({ boxes, main, scene, dragLatencies, setTimeframe, onDragMove }) };
