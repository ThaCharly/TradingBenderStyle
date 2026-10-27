// Criterio de #19: un fixture binario generado en Rust se decodifica en TS y se
// re-codifica a bytes idénticos. Los fixtures salen de crates/bender-types/tests/fixtures.rs.

import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { type DescMessage, fromBinary, toBinary } from "@bufbuild/protobuf";
import { describe, expect, it } from "vitest";
import { decimalToString } from "./decimal";
import { EnvelopeSchema } from "./gen/bender/v1/envelope_pb";
import { FillSchema, LedgerEntrySchema, OrderSchema } from "./gen/bender/v1/trading_pb";

const dir = new URL("../../../../../crates/bender-types/tests/fixtures/", import.meta.url);
const read = (name: string) => new Uint8Array(readFileSync(fileURLToPath(new URL(name, dir))));

const cases: [string, DescMessage][] = [
  ["envelope_trade.bin", EnvelopeSchema],
  ["order.bin", OrderSchema],
  ["fill.bin", FillSchema],
  ["ledger_entry.bin", LedgerEntrySchema],
];

describe("ida y vuelta Rust → TS → bytes", () => {
  for (const [name, schema] of cases) {
    it(name, () => {
      const bytes = read(name);
      const message = fromBinary(schema, bytes);
      expect(toBinary(schema, message)).toEqual(bytes);
    });
  }

  it("los valores decodificados son los que escribió Rust", () => {
    const env = fromBinary(EnvelopeSchema, read("envelope_trade.bin"));
    expect(env.ingestSeq).toBe(42n);
    expect(env.payload.case).toBe("market");
    const event = env.payload.case === "market" ? env.payload.value.event : undefined;
    expect(event?.case).toBe("trade");
    if (event?.case === "trade" && event.value.price) {
      expect(decimalToString(event.value.price)).toBe("88123.40");
    }
    const entry = fromBinary(LedgerEntrySchema, read("ledger_entry.bin"));
    expect(entry.amount && decimalToString(entry.amount)).toBe("-1180.591620717411315769");
  });
});
