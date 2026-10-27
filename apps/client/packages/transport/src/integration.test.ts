// Criterio de #21: el cliente TS consume StreamCounter contra el servidor Rust real
// (binario example-server de bender-transport), por gRPC-Web.

import { type ChildProcess, spawn } from "node:child_process";
import { createInterface } from "node:readline";
import { fileURLToPath } from "node:url";
import { createClient } from "@connectrpc/connect";
import { example } from "@bender/types";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import {
  createBenderTransport,
  IncompatibleVersionError,
  PROTOCOL_HEADER,
  parseVersion,
  resumeStream,
  toBenderError,
} from "./index";

const root = fileURLToPath(new URL("../../../../../", import.meta.url));
let server: ChildProcess;
let baseUrl: string;

beforeAll(async () => {
  server = spawn("cargo", ["run", "-q", "-p", "bender-transport", "--bin", "example-server"], {
    cwd: root,
    stdio: ["pipe", "pipe", "inherit"],
  });
  if (!server.stdout) throw new Error("sin stdout del servidor");
  const lines = createInterface({ input: server.stdout });
  baseUrl = await new Promise<string>((resolve, reject) => {
    server.once("exit", (code) => reject(new Error(`example-server salió con ${code}`)));
    lines.on("line", (line) => {
      const match = /^LISTENING (.+)$/.exec(line);
      if (match) resolve(`http://${match[1]}`);
    });
  });
}, 600_000);

afterAll(() => {
  server?.stdin?.end();
});

const v1 = { major: 1, minor: 0 };

describe("cliente TS ↔ servidor Rust (gRPC-Web)", () => {
  it("Echo devuelve el texto y la versión del servicio", async () => {
    const client = createClient(example.ExampleService, createBenderTransport({ baseUrl, version: v1 }));
    let serverVersion: string | null = null;
    const res = await client.echo(
      { text: "hola desde TS" },
      { onHeader: (h) => (serverVersion = h.get(PROTOCOL_HEADER)) },
    );
    expect(res.text).toBe("hola desde TS");
    expect(parseVersion(serverVersion)).toEqual(v1);
  });

  it("consume StreamCounter", async () => {
    const client = createClient(example.ExampleService, createBenderTransport({ baseUrl, version: v1 }));
    const seqs: bigint[] = [];
    for await (const ev of client.streamCounter({ afterIngestSeq: 5n, untilIngestSeq: 120n })) {
      expect(ev.value).toBe(ev.ingestSeq * ev.ingestSeq);
      seqs.push(ev.ingestSeq);
    }
    expect(seqs).toEqual(Array.from({ length: 115 }, (_, i) => BigInt(i + 6)));
  });

  it("reanuda por cursor después de cortes", async () => {
    const client = createClient(example.ExampleService, createBenderTransport({ baseUrl, version: v1 }));
    let reconnects = 0;
    // Simula un corte cada 7 eventos abortando la llamada desde el cliente.
    const open = (after: bigint | undefined) => {
      const controller = new AbortController();
      let n = 0;
      return (async function* () {
        for await (const ev of client.streamCounter(
          { afterIngestSeq: after, untilIngestSeq: 100n },
          { signal: controller.signal },
        )) {
          yield ev;
          if (++n === 7) controller.abort();
        }
      })();
    };
    const seqs: bigint[] = [];
    for await (const ev of resumeStream(open, { backoffMs: 0, onReconnect: () => reconnects++ })) {
      seqs.push(ev.ingestSeq);
    }
    expect(seqs).toEqual(Array.from({ length: 100 }, (_, i) => BigInt(i + 1)));
    expect(reconnects).toBeGreaterThanOrEqual(14);
  });

  it("una versión mayor incompatible da un error tipado", async () => {
    const client = createClient(
      example.ExampleService,
      createBenderTransport({ baseUrl, version: { major: 2, minor: 0 } }),
    );
    const err = await client.echo({ text: "x" }).catch(toBenderError);
    expect(err).toBeInstanceOf(IncompatibleVersionError);
    const mismatch = (err as IncompatibleVersionError).mismatch;
    expect(mismatch.service).toBe("bender.example.v1.ExampleService");
    expect(mismatch.server).toMatchObject({ major: 1, minor: 0 });
    expect(mismatch.client).toMatchObject({ major: 2, minor: 0 });
  });
});
