// Conversión del mensaje `bender.v1.Decimal` (mantisa i128 + escala) a texto y bigint.
// Un monto nunca pasa por `number`: perdería precisión pasado 2^53.

import { create } from "@bufbuild/protobuf";
import { type Decimal, DecimalSchema } from "./gen/bender/v1/decimal_pb";

const TWO_64 = 1n << 64n;
const I128_MIN = -(1n << 127n);
const I128_MAX = (1n << 127n) - 1n;
const MAX_SCALE = 38;

/** Mantisa exacta (entero con signo de 128 bits). */
export function decimalMantissa(d: Decimal): bigint {
  return d.mantissaHi * TWO_64 + d.mantissaLo;
}

/** Arma el mensaje desde mantisa y escala. Falla si no entra en 128 bits. */
export function decimalFromParts(mantissa: bigint, scale: number): Decimal {
  if (mantissa < I128_MIN || mantissa > I128_MAX) {
    throw new RangeError(`mantisa fuera de 128 bits: ${mantissa}`);
  }
  if (!Number.isInteger(scale) || scale < 0 || scale > MAX_SCALE) {
    throw new RangeError(`escala inválida: ${scale}`);
  }
  const lo = BigInt.asUintN(64, mantissa);
  const hi = BigInt.asIntN(64, mantissa >> 64n);
  return create(DecimalSchema, { mantissaLo: lo, mantissaHi: hi, scale });
}

/** Texto exacto, conservando la escala: mantisa 150, escala 2 → "1.50". */
export function decimalToString(d: Decimal): string {
  const m = decimalMantissa(d);
  const digits = (m < 0n ? -m : m).toString();
  const sign = m < 0n ? "-" : "";
  if (d.scale === 0) return sign + digits;
  const padded = digits.padStart(d.scale + 1, "0");
  const cut = padded.length - d.scale;
  return `${sign}${padded.slice(0, cut)}.${padded.slice(cut)}`;
}

/** Inverso de `decimalToString`. Acepta `[-+]dígitos[.dígitos]`. */
export function decimalFromString(s: string): Decimal {
  const match = /^([-+]?)(\d+)(?:\.(\d+))?$/.exec(s);
  if (!match) throw new SyntaxError(`decimal inválido: ${JSON.stringify(s)}`);
  const [, sign, int = "", frac = ""] = match;
  const magnitude = BigInt(int + frac);
  return decimalFromParts(sign === "-" ? -magnitude : magnitude, frac.length);
}
