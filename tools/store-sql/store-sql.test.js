// Criterio de #22: DuckDB lee los segmentos de bender-store con las consultas
// documentadas en docs/operacion/consultas-duckdb.md (se toman del documento, así no se
// desactualizan).

import { execFileSync } from "node:child_process";
import { mkdtempSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { DuckDBInstance } from "@duckdb/node-api";
import { afterAll, beforeAll, describe, expect, it } from "vitest";

const root = fileURLToPath(new URL("../../", import.meta.url));
const dir = mkdtempSync(join(tmpdir(), "bender-store-sql-"));
let connection;

function documentedQueries() {
  const doc = readFileSync(join(root, "docs/operacion/consultas-duckdb.md"), "utf8");
  return [...doc.matchAll(/```sql\n([\s\S]*?)```/g)].map((m) =>
    m[1].replaceAll("'datos/", `'${dir.replaceAll("\\", "/")}/`),
  );
}

async function rows(sql) {
  const reader = await connection.runAndReadAll(sql);
  return reader.getRowObjectsJson();
}

beforeAll(async () => {
  execFileSync("cargo", ["run", "-q", "-p", "bender-store", "--example", "write-sample", "--", dir], {
    cwd: root,
    stdio: "inherit",
  });
  const instance = await DuckDBInstance.create(":memory:");
  connection = await instance.connect();
}, 600_000);

afterAll(() => rmSync(dir, { recursive: true, force: true }));

describe("consultas de docs/operacion/consultas-duckdb.md", () => {
  it("el documento tiene las dos consultas", () => {
    expect(documentedQueries()).toHaveLength(2);
  });

  it("trades por fuente y día", async () => {
    const [sql] = documentedQueries();
    // 1.000 trades cada 2 min desde 2025-10-27 12:00 UTC: 359 antes de medianoche.
    expect(await rows(sql)).toEqual([
      { source: "BINANCE:PERP_LINEAR:BTCUSDT:trades", dia: "2025-10-27", trades: "359", primero: "1", ultimo: "359" },
      { source: "BINANCE:PERP_LINEAR:BTCUSDT:trades", dia: "2025-10-28", trades: "641", primero: "360", ultimo: "1000" },
      { source: "BINANCE:PERP_LINEAR:ETHUSDT:trades", dia: "2025-10-27", trades: "359", primero: "1", ultimo: "359" },
      { source: "BINANCE:PERP_LINEAR:ETHUSDT:trades", dia: "2025-10-28", trades: "641", primero: "360", ultimo: "1000" },
    ]);
  });

  it("mensajes crudos por fuente", async () => {
    const [, sql] = documentedQueries();
    const result = await rows(sql);
    expect(result.map((r) => [r.source, r.mensajes])).toEqual([
      ["BINANCE:PERP_LINEAR:BTCUSDT:trades", "1000"],
      ["BINANCE:PERP_LINEAR:ETHUSDT:trades", "1000"],
    ]);
    expect(Number(result[0].bytes)).toBeGreaterThan(50_000);
  });
});
