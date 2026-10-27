// Criterio de #19: en TS ningún monto se expone como `number`. Los montos son mensajes
// `Decimal` (bigint + escala); acá se verifica que ningún campo del esquema sea float y
// que los enteros de 64 bits salgan como bigint. Los únicos `number` que quedan son
// enteros de 32 bits (escala, versión de ticket) y enums.

import { ScalarType } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";
import * as types from "./index";

const FLOATS = [ScalarType.DOUBLE, ScalarType.FLOAT];
const INT64 = [
  ScalarType.INT64,
  ScalarType.UINT64,
  ScalarType.SINT64,
  ScalarType.FIXED64,
  ScalarType.SFIXED64,
];

describe("ningún monto como number", () => {
  it("sin floats y con los enteros de 64 bits como bigint", () => {
    const offenders: string[] = [];
    let checked = 0;
    for (const value of Object.values(types)) {
      if (typeof value !== "object" || value === null || !("kind" in value)) continue;
      if (value.kind !== "message") continue;
      for (const field of value.fields) {
        checked++;
        if (field.fieldKind !== "scalar" && field.fieldKind !== "list") continue;
        const scalar = field.scalar;
        if (scalar === undefined) continue;
        if (FLOATS.includes(scalar) || (INT64.includes(scalar) && field.longAsString)) {
          offenders.push(`${value.typeName}.${field.name}`);
        }
      }
    }
    expect(checked).toBeGreaterThan(100);
    expect(offenders).toEqual([]);
  });
});
