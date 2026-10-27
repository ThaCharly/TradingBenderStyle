// Cliente TS de la capa de transporte (#21): Connect-Web sobre gRPC-Web contra los
// servidores de bender-transport, con versionado y streams reanudables por cursor.

import { Code, ConnectError, type Interceptor, type Transport } from "@connectrpc/connect";
import { createGrpcWebTransport } from "@connectrpc/connect-web";
import { type ProtocolVersion, type VersionMismatch, VersionMismatchSchema } from "@bender/types";

/** Metadato con la versión de protocolo ("MAYOR.MENOR"), igual que en Rust. */
export const PROTOCOL_HEADER = "bender-protocol-version";

export interface Version {
  major: number;
  minor: number;
}

export type Compatibility = "same" | "minor-skew" | "incompatible";

/** Misma regla que el servidor: misma mayor y menores a distancia ≤ 1. */
export function compatibility(service: Version, client: Version): Compatibility {
  if (service.major !== client.major || Math.abs(service.minor - client.minor) > 1) {
    return "incompatible";
  }
  return service.minor === client.minor ? "same" : "minor-skew";
}

export function parseVersion(text: string | null | undefined): Version | undefined {
  const match = /^(\d+)\.(\d+)$/.exec(text?.trim() ?? "");
  if (!match) return undefined;
  return { major: Number(match[1]), minor: Number(match[2]) };
}

/** Agrega la versión del cliente a cada llamada. */
export function versionInterceptor(version: Version): Interceptor {
  return (next) => (req) => {
    req.header.set(PROTOCOL_HEADER, `${version.major}.${version.minor}`);
    return next(req);
  };
}

export interface TransportOptions {
  /** URL del servidor, p. ej. "http://localhost:7400". */
  baseUrl: string;
  /** Versión de protocolo que habla este cliente. */
  version: Version;
  fetch?: typeof globalThis.fetch;
}

/** Transporte gRPC-Web con la versión del cliente incluida en cada llamada. */
export function createBenderTransport(options: TransportOptions): Transport {
  return createGrpcWebTransport({
    baseUrl: options.baseUrl,
    interceptors: [versionInterceptor(options.version)],
    ...(options.fetch ? { fetch: options.fetch } : {}),
  });
}

/** La versión del cliente no es compatible con la del servicio. */
export class IncompatibleVersionError extends Error {
  override readonly name = "IncompatibleVersionError";
  constructor(
    readonly mismatch: VersionMismatch,
    override readonly cause: ConnectError,
  ) {
    super(cause.rawMessage);
  }

  get server(): ProtocolVersion | undefined {
    return this.mismatch.server;
  }
}

/** Convierte los errores de versión en `IncompatibleVersionError`; el resto, igual. */
export function toBenderError(err: unknown): unknown {
  const connectErr = ConnectError.from(err);
  if (connectErr.code === Code.FailedPrecondition) {
    const [mismatch] = connectErr.findDetails(VersionMismatchSchema);
    if (mismatch) return new IncompatibleVersionError(mismatch, connectErr);
  }
  return err;
}

const RETRYABLE = new Set([
  Code.Unavailable,
  Code.Unknown,
  Code.Internal,
  Code.Canceled,
  Code.Aborted,
  Code.DeadlineExceeded,
]);

export interface ResumeOptions {
  /** Último cursor ya recibido; ausente = desde el principio. */
  after?: bigint;
  /** Espera antes de reconectar, en ms. */
  backoffMs?: number;
  /** Reconexiones seguidas sin recibir eventos antes de rendirse. */
  maxAttemptsWithoutProgress?: number;
  /** Se llama en cada reconexión (para mostrar "reconectando…"). */
  onReconnect?: (error: unknown) => void;
}

/**
 * Stream reanudable por cursor, como `resume::resumable` en Rust: ante un corte
 * reintentable, vuelve a abrir con el último `ingestSeq` recibido y descarta
 * duplicados. Los errores no reintentables (incluida una versión incompatible) se lanzan.
 */
export async function* resumeStream<T extends { ingestSeq: bigint }>(
  open: (after: bigint | undefined) => AsyncIterable<T>,
  options: ResumeOptions = {},
): AsyncGenerator<T> {
  const backoff = options.backoffMs ?? 200;
  const maxAttempts = options.maxAttemptsWithoutProgress ?? 10;
  let last = options.after;
  let attempts = 0;
  for (;;) {
    const before = last;
    try {
      for await (const event of open(last)) {
        if (last !== undefined && event.ingestSeq <= last) continue;
        last = event.ingestSeq;
        yield event;
      }
      return;
    } catch (err) {
      const error = toBenderError(err);
      const retryable =
        !(error instanceof IncompatibleVersionError) &&
        RETRYABLE.has(ConnectError.from(error).code);
      attempts = last === before ? attempts + 1 : 1;
      if (!retryable || attempts > maxAttempts) throw error;
      options.onReconnect?.(error);
      await new Promise((resolve) => setTimeout(resolve, backoff));
    }
  }
}
