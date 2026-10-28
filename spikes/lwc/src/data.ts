// Datos sintéticos deterministas y agregación de temporalidades.

import type { CandlestickData, UTCTimestamp } from "lightweight-charts";

export interface Bar {
  time: number; // segundos UTC
  open: number;
  high: number;
  low: number;
  close: number;
  volume: number;
}

/** Generador pseudoaleatorio determinista (mulberry32). */
export function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

/** `n` velas de 1 s terminando en `end` (segundos), paseo aleatorio alrededor de 80.000. */
export function syntheticBars(n: number, end: number, seed = 44): Bar[] {
  const r = rng(seed);
  const bars: Bar[] = new Array(n);
  let price = 80_000;
  const start = end - n + 1;
  for (let i = 0; i < n; i++) {
    const open = price;
    let high = open;
    let low = open;
    for (let k = 0; k < 4; k++) {
      price += (r() - 0.5) * 6;
      high = Math.max(high, price);
      low = Math.min(low, price);
    }
    bars[i] = {
      time: start + i,
      open,
      high,
      low,
      close: price,
      volume: 0.01 + r() * r() * 3,
    };
  }
  return bars;
}

/** Agrega velas de 1 s a `tf` segundos (alineadas al epoch). */
export function aggregate(bars: Bar[], tf: number): Bar[] {
  if (tf === 1) return bars;
  const out: Bar[] = [];
  let cur: Bar | null = null;
  for (const b of bars) {
    const t = Math.floor(b.time / tf) * tf;
    if (!cur || cur.time !== t) {
      if (cur) out.push(cur);
      cur = { ...b, time: t };
    } else {
      cur.high = Math.max(cur.high, b.high);
      cur.low = Math.min(cur.low, b.low);
      cur.close = b.close;
      cur.volume += b.volume;
    }
  }
  if (cur) out.push(cur);
  return out;
}

export function toSeries(bars: Bar[]): CandlestickData<UTCTimestamp>[] {
  return bars.map((b) => ({
    time: b.time as UTCTimestamp,
    open: b.open,
    high: b.high,
    low: b.low,
    close: b.close,
  }));
}

/** Índice lógico (fraccionario) de un instante: dentro de los datos interpola entre
 * velas; a la derecha de la última extrapola con la temporalidad. Así un dibujo anclado
 * en el futuro (en tiempo de dominio) queda en el mismo instante en cualquier TF. */
export function timeToLogical(bars: Bar[], tf: number, t: number): number {
  const n = bars.length;
  if (n === 0) return 0;
  const last = bars[n - 1].time;
  if (t >= last) return n - 1 + (t - last) / tf;
  const first = bars[0].time;
  if (t <= first) return (t - first) / tf;
  let lo = 0;
  let hi = n - 1;
  while (hi - lo > 1) {
    const mid = (lo + hi) >> 1;
    if (bars[mid].time <= t) lo = mid;
    else hi = mid;
  }
  const span = bars[hi].time - bars[lo].time;
  return lo + (span > 0 ? (t - bars[lo].time) / span : 0);
}

/** Inverso de {@link timeToLogical}. */
export function logicalToTime(bars: Bar[], tf: number, logical: number): number {
  const n = bars.length;
  const last = bars[n - 1].time;
  if (logical >= n - 1) return last + (logical - (n - 1)) * tf;
  if (logical <= 0) return bars[0].time + logical * tf;
  const lo = Math.floor(logical);
  const frac = logical - lo;
  return bars[lo].time + frac * (bars[lo + 1].time - bars[lo].time);
}
