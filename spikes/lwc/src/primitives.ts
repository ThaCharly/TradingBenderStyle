// Primitivas del spike: dibujos en coordenadas de dominio (tiempo UTC en segundos y
// precio), convertidas a píxeles en cada cuadro. No guardan coordenadas de pantalla.

import type {
  IChartApi,
  IPanePrimitive,
  IPrimitivePaneRenderer,
  IPrimitivePaneView,
  ISeriesApi,
  ISeriesPrimitive,
  ISeriesPrimitiveAxisView,
  Logical,
  PaneAttachedParameter,
  PrimitivePaneViewZOrder,
  SeriesAttachedParameter,
  SeriesType,
  Time,
} from "lightweight-charts";
import type { CanvasRenderingTarget2D } from "fancy-canvas";

import { type Bar, timeToLogical } from "./data";

/** Temporalidad y velas que muestra una gráfica (cambia al cambiar de TF). */
export interface ChartFrame {
  bars: Bar[];
  tf: number;
}

export interface Point {
  time: number;
  price: number;
}

/** Conversión dominio → píxeles de una gráfica. */
export class Projector {
  chart: IChartApi | null = null;
  series: ISeriesApi<SeriesType> | null = null;
  constructor(public frame: ChartFrame) {}

  x(time: number): number | null {
    if (!this.chart) return null;
    return logicalX(this.chart, timeToLogical(this.frame.bars, this.frame.tf, time));
  }

  y(price: number): number | null {
    return this.series?.priceToCoordinate(price) ?? null;
  }
}

/** `logicalToCoordinate` de Lightweight Charts sólo acepta índices enteros (con uno
 * fraccionario devuelve 0): se interpola entre los dos enteros vecinos. */
export function logicalX(chart: IChartApi, logical: number): number | null {
  const ts = chart.timeScale();
  const lo = Math.floor(logical);
  const x0 = ts.logicalToCoordinate(lo as Logical);
  if (x0 === null) return null;
  const frac = logical - lo;
  if (frac === 0) return x0;
  const x1 = ts.logicalToCoordinate((lo + 1) as Logical);
  return x1 === null ? null : x0 + frac * (x1 - x0);
}

type Draw = (ctx: CanvasRenderingContext2D, w: number, h: number) => void;

class FnRenderer implements IPrimitivePaneRenderer {
  constructor(private fn: Draw, private background = false) {}
  draw(target: CanvasRenderingTarget2D): void {
    if (this.background) return;
    target.useMediaCoordinateSpace(({ context, mediaSize }) =>
      this.fn(context, mediaSize.width, mediaSize.height),
    );
  }
  drawBackground(target: CanvasRenderingTarget2D): void {
    if (!this.background) return;
    target.useMediaCoordinateSpace(({ context, mediaSize }) =>
      this.fn(context, mediaSize.width, mediaSize.height),
    );
  }
}

class FnView implements IPrimitivePaneView {
  private r: FnRenderer;
  constructor(fn: Draw, private z: PrimitivePaneViewZOrder = "normal", background = false) {
    this.r = new FnRenderer(fn, background);
  }
  zOrder(): PrimitivePaneViewZOrder {
    return this.z;
  }
  renderer(): IPrimitivePaneRenderer {
    return this.r;
  }
}

/** Base de las primitivas de serie: guarda la referencia para pedir redibujo. */
abstract class Base implements ISeriesPrimitive<Time> {
  protected requestUpdate: (() => void) | null = null;
  protected views: readonly IPrimitivePaneView[];
  /** Versión del estado y última versión pintada (para medir evento → cuadro pintado). */
  version = 0;
  painted = 0;
  constructor(protected p: Projector, z: PrimitivePaneViewZOrder = "normal") {
    this.views = [
      new FnView((c, w, h) => {
        this.painted = this.version;
        this.paint(c, w, h);
      }, z),
    ];
  }
  attached(param: SeriesAttachedParameter<Time>): void {
    this.requestUpdate = param.requestUpdate;
  }
  detached(): void {
    this.requestUpdate = null;
  }
  paneViews(): readonly IPrimitivePaneView[] {
    return this.views;
  }
  update(): void {
    this.version++;
    this.requestUpdate?.();
  }
  abstract paint(ctx: CanvasRenderingContext2D, w: number, h: number): void;
}

/** Nivel horizontal; si es orden, es arrastrable y tiene etiqueta en el eje de precio. */
export class HLine extends Base {
  private axis: readonly ISeriesPrimitiveAxisView[];
  constructor(
    p: Projector,
    public price: number,
    public color: string,
    public label: string | null,
    public dashed = false,
  ) {
    super(p);
    const self = this;
    this.axis = label
      ? [
          {
            coordinate: () => self.p.y(self.price) ?? -100,
            text: () => `${self.label} ${self.price.toFixed(1)}`,
            textColor: () => "#fff",
            backColor: () => self.color,
          },
        ]
      : [];
  }
  priceAxisViews(): readonly ISeriesPrimitiveAxisView[] {
    return this.axis;
  }
  paint(ctx: CanvasRenderingContext2D, w: number): void {
    const y = this.p.y(this.price);
    if (y === null) return;
    ctx.strokeStyle = this.color;
    ctx.lineWidth = this.label ? 1.5 : 1;
    ctx.setLineDash(this.dashed ? [6, 4] : []);
    ctx.beginPath();
    ctx.moveTo(0, Math.round(y) + 0.5);
    ctx.lineTo(w, Math.round(y) + 0.5);
    ctx.stroke();
    ctx.setLineDash([]);
  }
  hit(_x: number, y: number): boolean {
    const py = this.p.y(this.price);
    return py !== null && Math.abs(py - y) <= 6;
  }
}

/** Línea de tendencia entre dos puntos de dominio. */
export class TrendLine extends Base {
  constructor(p: Projector, public a: Point, public b: Point, public color = "#2962ff") {
    super(p);
  }
  paint(ctx: CanvasRenderingContext2D): void {
    const ax = this.p.x(this.a.time);
    const ay = this.p.y(this.a.price);
    const bx = this.p.x(this.b.time);
    const by = this.p.y(this.b.price);
    if (ax === null || ay === null || bx === null || by === null) return;
    ctx.strokeStyle = this.color;
    ctx.lineWidth = 2;
    ctx.beginPath();
    ctx.moveTo(ax, ay);
    ctx.lineTo(bx, by);
    ctx.stroke();
    ctx.fillStyle = this.color;
    for (const [x, y] of [
      [ax, ay],
      [bx, by],
    ]) {
      ctx.beginPath();
      ctx.arc(x, y, 4, 0, Math.PI * 2);
      ctx.fill();
    }
  }
  /** "a", "b" (extremos), "body" o null. */
  hit(x: number, y: number): "a" | "b" | "body" | null {
    const ax = this.p.x(this.a.time);
    const ay = this.p.y(this.a.price);
    const bx = this.p.x(this.b.time);
    const by = this.p.y(this.b.price);
    if (ax === null || ay === null || bx === null || by === null) return null;
    if (Math.hypot(x - ax, y - ay) <= 8) return "a";
    if (Math.hypot(x - bx, y - by) <= 8) return "b";
    const len2 = (bx - ax) ** 2 + (by - ay) ** 2;
    const t = len2 > 0 ? Math.max(0, Math.min(1, ((x - ax) * (bx - ax) + (y - ay) * (by - ay)) / len2)) : 0;
    const d = Math.hypot(x - (ax + t * (bx - ax)), y - (ay + t * (by - ay)));
    return d <= 6 ? "body" : null;
  }
}

/** Rectángulo entre dos esquinas de dominio. */
export class Rect extends Base {
  constructor(
    p: Projector,
    public a: Point,
    public b: Point,
    public color = "rgba(255,152,0,0.18)",
    public border = "#ff9800",
  ) {
    super(p);
  }
  box(): [number, number, number, number] | null {
    const ax = this.p.x(this.a.time);
    const ay = this.p.y(this.a.price);
    const bx = this.p.x(this.b.time);
    const by = this.p.y(this.b.price);
    if (ax === null || ay === null || bx === null || by === null) return null;
    return [Math.min(ax, bx), Math.min(ay, by), Math.abs(bx - ax), Math.abs(by - ay)];
  }
  paint(ctx: CanvasRenderingContext2D): void {
    const b = this.box();
    if (!b) return;
    ctx.fillStyle = this.color;
    ctx.fillRect(...b);
    ctx.strokeStyle = this.border;
    ctx.lineWidth = 1.5;
    ctx.strokeRect(...b);
  }
  /** "b" (esquina derecha-inferior para redimensionar), "body" o null. */
  hit(x: number, y: number): "b" | "body" | null {
    const bx = this.p.x(this.b.time);
    const by = this.p.y(this.b.price);
    if (bx !== null && by !== null && Math.hypot(x - bx, y - by) <= 8) return "b";
    const b = this.box();
    if (!b) return null;
    return x >= b[0] && x <= b[0] + b[2] && y >= b[1] && y <= b[1] + b[3] ? "body" : null;
  }
}

/** Perfil de volumen anclado a un rango de tiempo, como primitiva de **panel**. */
export class VolumeProfile implements IPanePrimitive<Time> {
  private views: readonly IPrimitivePaneView[];
  private requestUpdate: (() => void) | null = null;
  private bins: { price: number; volume: number }[] = [];
  private binSize = 0;
  private maxVolume = 0;
  constructor(
    private p: Projector,
    public from: number,
    public to: number,
    private rows = 60,
    private color = "rgba(38,166,154,0.45)",
  ) {
    this.views = [new FnView((c) => this.paint(c), "bottom")];
    this.compute();
  }
  compute(): void {
    const bars = this.p.frame.bars.filter((b) => b.time >= this.from && b.time < this.to);
    if (bars.length === 0) {
      this.bins = [];
      return;
    }
    let lo = Infinity;
    let hi = -Infinity;
    for (const b of bars) {
      lo = Math.min(lo, b.low);
      hi = Math.max(hi, b.high);
    }
    this.binSize = (hi - lo) / this.rows || 1;
    const vols = new Array(this.rows).fill(0);
    for (const b of bars) {
      const i = Math.min(this.rows - 1, Math.floor(((b.high + b.low) / 2 - lo) / this.binSize));
      vols[i] += b.volume;
    }
    this.maxVolume = Math.max(...vols);
    this.bins = vols.map((volume, i) => ({ price: lo + (i + 0.5) * this.binSize, volume }));
  }
  attached(param: PaneAttachedParameter<Time>): void {
    this.requestUpdate = param.requestUpdate;
  }
  paneViews(): readonly IPrimitivePaneView[] {
    return this.views;
  }
  update(): void {
    this.requestUpdate?.();
  }
  private paint(ctx: CanvasRenderingContext2D): void {
    const x0 = this.p.x(this.from);
    const x1 = this.p.x(this.to);
    if (x0 === null || x1 === null || this.maxVolume === 0) return;
    const width = Math.max(0, x1 - x0);
    ctx.fillStyle = this.color;
    for (const bin of this.bins) {
      const yTop = this.p.y(bin.price + this.binSize / 2);
      const yBot = this.p.y(bin.price - this.binSize / 2);
      if (yTop === null || yBot === null) continue;
      const w = (bin.volume / this.maxVolume) * width * 0.6;
      ctx.fillRect(x0, yTop, w, Math.max(1, yBot - yTop - 1));
    }
  }
}

/** Heatmap del libro: una grilla `cols × rows` en un bitmap fuera de pantalla usado como
 * búfer circular por columnas (cada actualización del libro escribe una columna) y dibujado
 * con `drawImage` dentro de la primitiva. */
export class Heatmap extends Base {
  readonly canvas: OffscreenCanvas | HTMLCanvasElement;
  private ctx2d: OffscreenCanvasRenderingContext2D | CanvasRenderingContext2D;
  private column: ImageData;
  private palette = new Uint32Array(256);
  /** Próxima columna a escribir del búfer circular (= columnas escritas). */
  private head = 0;
  enabled = false;
  /** Último tiempo de actualización (ms) y de dibujo (ms). */
  composeMs = 0;
  drawMs = 0;
  constructor(
    p: Projector,
    public cols = 1000,
    public rows = 500,
    public priceLo = 0,
    public priceHi = 0,
  ) {
    super(p, "bottom");
    this.canvas =
      typeof OffscreenCanvas !== "undefined"
        ? new OffscreenCanvas(cols, rows)
        : Object.assign(document.createElement("canvas"), { width: cols, height: rows });
    this.ctx2d = this.canvas.getContext("2d") as CanvasRenderingContext2D;
    this.column = new ImageData(1, rows);
    for (let v = 0; v < 256; v++) {
      // ABGR (little endian): de azul (poca liquidez) a amarillo (mucha).
      const r = v;
      const g = Math.min(255, v * 1.6) * 0.8;
      const b = 255 - v;
      this.palette[v] = (150 << 24) | (b << 16) | (g << 8) | r;
    }
  }
  /** Valores (índices de paleta → colores) de un snapshot del libro sintético en la columna
   * `col` de `px` (con `stride` píxeles por fila). */
  private fillColumn(px: Uint32Array, stride: number, col: number, seed: number): void {
    const center = this.rows / 2 + Math.sin(seed * 0.013) * 90 + Math.sin(seed * 0.071) * 20;
    for (let r = 0; r < this.rows; r++) {
      const d = Math.abs(r - center);
      const noise = ((Math.imul(r, 2654435761) ^ Math.imul(seed >> 3, 40503)) >>> 26) & 31;
      px[r * stride + col] = this.palette[Math.max(0, Math.min(255, 255 - d * 2.2 - noise))];
    }
  }
  /** Escribe la columna de un snapshot nuevo en el búfer circular. */
  private writeColumn(seed: number): void {
    this.fillColumn(new Uint32Array(this.column.data.buffer), 1, 0, seed);
    this.ctx2d.putImageData(this.column, this.head % this.cols, 0);
    this.head++;
  }
  /** Recompone todo el bitmap (p. ej. al cambiar el rango de precios): una sola
   * `putImageData`. */
  composeAll(): number {
    const t0 = performance.now();
    const image = new ImageData(this.cols, this.rows);
    const px = new Uint32Array(image.data.buffer);
    for (let c = 0; c < this.cols; c++) this.fillColumn(px, this.cols, c, c);
    this.ctx2d.putImageData(image, 0, 0);
    this.head = this.cols;
    return performance.now() - t0;
  }
  /** Una actualización del libro: una columna nueva. */
  compose(): void {
    const t0 = performance.now();
    this.writeColumn(this.head);
    this.composeMs = performance.now() - t0;
  }
  paint(ctx: CanvasRenderingContext2D, w: number): void {
    if (!this.enabled) return;
    const t0 = performance.now();
    const n = this.p.frame.bars.length;
    const last = this.p.frame.bars[n - 1];
    const firstTime = this.p.frame.bars[Math.max(0, n - this.cols)].time;
    const x0 = this.p.x(firstTime) ?? 0;
    const x1 = this.p.x(last.time) ?? w;
    const yTop = this.p.y(this.priceHi);
    const yBot = this.p.y(this.priceLo);
    if (yTop === null || yBot === null) return;
    ctx.imageSmoothingEnabled = false;
    // Búfer circular: la columna más vieja está en `head % cols`.
    const split = this.head % this.cols;
    const colW = Math.max(1, x1 - x0) / this.cols;
    const img = this.canvas as CanvasImageSource;
    const left = this.cols - split;
    ctx.drawImage(img, split, 0, left, this.rows, x0, yTop, left * colW, yBot - yTop);
    if (split > 0) ctx.drawImage(img, 0, 0, split, this.rows, x0 + left * colW, yTop, split * colW, yBot - yTop);
    this.drawMs = performance.now() - t0;
  }
}
