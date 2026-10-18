// Answering extension points (design note 0029, §3, §15): Hypha POSTs the point's request
// as JSON with `Hypha-Signature`, `Hypha-Extension`, `Hypha-Extension-Version`,
// `Idempotency-Key` and `Hypha-Store`, and takes a 2xx whose body matches the point's
// response. `extensionHandler` is a fetch handler (Workers' `fetch`, Hono, Next.js route
// handlers, Node through any Request/Response adapter) that checks the signature and
// dispatches by `Hypha-Extension` to the handlers defined with `defineExtensionHandler`.

import type { ExtensionPoints } from "./generated/extensions.ts";
import { DEFAULT_TOLERANCE_SECONDS, SIGNATURE_HEADER, verifySignature } from "./signature.ts";

export type PointName = keyof ExtensionPoints;
export type PointRequest<P extends PointName> = ExtensionPoints[P]["request"];
export type PointResponse<P extends PointName> = ExtensionPoints[P]["response"];

export const EXTENSION_HEADER = "Hypha-Extension";
export const EXTENSION_VERSION_HEADER = "Hypha-Extension-Version";
export const IDEMPOTENCY_HEADER = "Idempotency-Key";
export const STORE_HEADER = "Hypha-Store";

/** What a call carries besides its body. */
export interface ExtensionContext<P extends PointName = PointName> {
  point: P;
  version: number;
  /** Retries of the same call repeat it: answer them alike. */
  idempotencyKey: string | undefined;
  /** The calling store's ID. */
  store: string | undefined;
  /** The raw request, for anything else. */
  request: Request;
}

export type ExtensionHandlerFn<P extends PointName> = (
  body: PointRequest<P>,
  context: ExtensionContext<P>,
) => PointResponse<P> | Promise<PointResponse<P>>;

export interface ExtensionHandler<P extends PointName = PointName> {
  point: P;
  /** The versions this handler answers. */
  versions: readonly number[];
  handle: ExtensionHandlerFn<P>;
}

/** One point's handler, typed by the point's request and response. */
export function defineExtensionHandler<P extends PointName>(
  point: P,
  handle: ExtensionHandlerFn<P>,
  options: { versions?: readonly number[] } = {},
): ExtensionHandler<P> {
  return { point, versions: options.versions ?? [1], handle };
}

/** Thrown from a handler to answer a specific error instead of a 500. */
export class ExtensionError extends Error {
  readonly code: string;
  readonly status: number;
  constructor(code: string, message: string, status = 422) {
    super(message);
    this.name = "ExtensionError";
    this.code = code;
    this.status = status;
  }
}

export interface ExtensionHandlerOptions {
  /** The app's secret (from the first token exchange). A function for apps with several installations. */
  secret: string | ((request: Request) => string | undefined | Promise<string | undefined>);
  // A handler per point; `ExtensionHandler<any>` keeps each one's own types.
  // oxlint-disable-next-line typescript/no-explicit-any -- a list of handlers for different points
  handlers: readonly ExtensionHandler<any>[];
  /** Seconds a signature's timestamp may be off. Default 300. */
  toleranceSeconds?: number;
  /** Called with a handler's unexpected error (it's answered as a 500 without details). */
  onError?: (error: unknown, context: ExtensionContext) => void;
}

/**
 * A fetch handler for every point the app implements. Errors answer
 * `{code, error}`: 401 `invalid_signature`, 404 `unknown_extension`, 400
 * `unsupported_version` or `invalid_request`, 405 `method_not_allowed`, a thrown
 * `ExtensionError`'s status and code, else 500 `handler_failed`.
 */
export function extensionHandler(
  options: ExtensionHandlerOptions,
): (request: Request) => Promise<Response> {
  const byPoint = new Map(options.handlers.map((handler) => [handler.point, handler]));
  return async (request) => {
    if (request.method !== "POST") return failure(405, "method_not_allowed", "use POST");
    const raw = new Uint8Array(await request.arrayBuffer());
    const secret =
      typeof options.secret === "function" ? await options.secret(request) : options.secret;
    const signed =
      secret !== undefined &&
      (await verifySignature(secret, request.headers.get(SIGNATURE_HEADER), raw, {
        toleranceSeconds: options.toleranceSeconds ?? DEFAULT_TOLERANCE_SECONDS,
      }));
    if (!signed) return failure(401, "invalid_signature", "the signature doesn't verify");

    const point = request.headers.get(EXTENSION_HEADER) ?? "";
    const handler = byPoint.get(point);
    if (!handler)
      return failure(404, "unknown_extension", `no handler for ${JSON.stringify(point)}`);
    const version = Number(request.headers.get(EXTENSION_VERSION_HEADER) ?? "1");
    if (!handler.versions.includes(version))
      return failure(400, "unsupported_version", `${point} v${version} isn't supported`);

    let body: unknown;
    try {
      body = JSON.parse(new TextDecoder().decode(raw));
    } catch {
      return failure(400, "invalid_request", "the body isn't JSON");
    }
    const context: ExtensionContext = {
      point: handler.point,
      version,
      idempotencyKey: request.headers.get(IDEMPOTENCY_HEADER) ?? undefined,
      store: request.headers.get(STORE_HEADER) ?? undefined,
      request,
    };
    try {
      const answer: unknown = await handler.handle(body, context);
      return Response.json(answer);
    } catch (error) {
      if (error instanceof ExtensionError) return failure(error.status, error.code, error.message);
      options.onError?.(error, context);
      return failure(500, "handler_failed", "the handler failed");
    }
  };
}

function failure(status: number, code: string, error: string): Response {
  return Response.json({ code, error }, { status });
}
