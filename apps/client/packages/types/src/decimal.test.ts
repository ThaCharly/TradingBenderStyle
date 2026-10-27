import { describe, expect, it } from "vitest";
import {
  decimalFromParts,
  decimalFromString,
  decimalMantissa,
  decimalToString,
} from "./decimal";

describe("Decimal", () => {
  it("ida y vuelta por texto conservando la escala", () => {
    for (const s of ["0", "1.50", "-0.001", "88000.12345678", "-12", "0.000000000000000001"]) {
      expect(decimalToString(decimalFromString(s))).toBe(s);
    }
  });

  it("usa la mantisa de 128 bits en complemento a dos, igual que Rust", () => {
    const minusOne = decimalFromString("-1");
    expect([minusOne.mantissaHi, minusOne.mantissaLo]).toEqual([-1n, (1n << 64n) - 1n]);
    const big = decimalFromParts(-(1n << 70n) - 12345n, 18);
    expect(decimalMantissa(big)).toBe(-(1n << 70n) - 12345n);
  });

  it("rechaza texto inválido y valores fuera de rango", () => {
    for (const bad of ["", "-", ".5", "5.", "1e5", " 1"]) {
      expect(() => decimalFromString(bad)).toThrow();
    }
    expect(() => decimalFromParts(1n << 127n, 0)).toThrow(RangeError);
    expect(() => decimalFromParts(1n, 39)).toThrow(RangeError);
  });
});
