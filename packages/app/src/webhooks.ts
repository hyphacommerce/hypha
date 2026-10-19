// Receiving Hypha's webhooks (design notes 0016 §8, 0029 §4.1): Hypha POSTs an event's
// envelope as JSON, signed in `Hypha-Signature`, with `Hypha-Event-Id`,
// `Hypha-Event-Type`, `Hypha-Delivery-Id` (stable across retries) and `Hypha-Store`. Any
// 2xx within the timeout is a success; anything else is retried on Hypha's schedule.
// `webhookHandler` is a fetch handler that verifies the signature, parses the envelope
// and hands it to `onEvent`.

import { STORE_HEADER, resolveSecret, type SecretSource } from "./extensions.ts";
import { isRecord } from "./encoding.ts";
import { DEFAULT_TOLERANCE_SECONDS, SIGNATURE_HEADER, verifySignature } from "./signature.ts";

export const EVENT_ID_HEADER = "Hypha-Event-Id";
export const EVENT_TYPE_HEADER = "Hypha-Event-Type";
export const DELIVERY_ID_HEADER = "Hypha-Delivery-Id";

/** Who caused an event. */
export interface WebhookActor {
  type: "user" | "agent" | "api_key" | "app" | "system";
  /** Their prefixed ID; `null` for the system. */
  id: string | null;
}

/** What an event is about. */
export interface WebhookSubject {
  /** `product`, `order`, `app`, … */
  type: string;
  /**
   * Its prefixed ID (`prod_…`, `chan_…`, `appid_…`); a kind without an ID type (the
   * store's one `search_settings`) has the bare UUID.
   */
  id: string;
  /** Its version after the change, when it has one: drop an event older than one applied. */
  version: number | null;
}

/** An event as Hypha delivers it (and as `GET /admin/events/{id}` shows it). */
export interface WebhookEnvelope<D = Record<string, unknown>> {
  /** `evt_…`, the same in every delivery and retry of the event. */
  id: string;
  /** Dotted: `order.placed`, `app.installed`. */
  type: string;
  /** The payload's version: a reshaped payload is a new version of the same type. */
  version: number;
  subject: WebhookSubject;
  actor: WebhookActor;
  /** When the change committed, RFC 3339. */
  occurred_at: string;
  /** The facts of the change (see the event catalog); personal data only with its scopes. */
  data: D;
}

/** What a delivery carries besides its envelope. */
export interface WebhookContext {
  /** `dlv_…`: stable across retries of this delivery, for de-duplicating. */
  deliveryId: string;
  /** `store_…` from `Hypha-Store`. */
  store: string | undefined;
  /** The raw request, for anything else. */
  request: Request;
}

/** Remembers deliveries already handled, so a retry isn't handled twice. */
export interface WebhookDedupe {
  /** Whether the delivery was handled before: then it's answered 200 without `onEvent`. */
  seen(deliveryId: string, context: WebhookContext): boolean | Promise<boolean>;
  /** Called after `onEvent` succeeded. */
  remember(deliveryId: string, context: WebhookContext): void | Promise<void>;
}

export interface WebhookHandlerOptions {
  /** The app's secret, or a lookup by `store` for apps with several installations. */
  secret: SecretSource;
  /** Handles one event. Throwing answers 500, so Hypha retries the delivery later. */
  onEvent: (event: WebhookEnvelope, context: WebhookContext) => void | Promise<void>;
  dedupe?: WebhookDedupe;
  /** Seconds a signature's timestamp may be off. Default 300. */
  toleranceSeconds?: number;
  /** Called with `onEvent`'s error (it's answered as a 500 without details). */
  onError?: (error: unknown, event: WebhookEnvelope, context: WebhookContext) => void;
}

/**
 * A fetch handler for the manifest's `webhooks.url`. Answers 204 when handled (or a
 * duplicate), 401 `invalid_signature`, 400 `invalid_request` for a body that isn't an
 * envelope, 405 for anything but POST, 500 `handler_failed` when `onEvent` throws.
 */
export function webhookHandler(
  options: WebhookHandlerOptions,
): (request: Request) => Promise<Response> {
  return async (request) => {
    if (request.method !== "POST") return failure(405, "method_not_allowed", "use POST");
    const raw = new Uint8Array(await request.arrayBuffer());
    const store = request.headers.get(STORE_HEADER) ?? undefined;
    const secret = await resolveSecret(options.secret, { request, rawBody: raw, store });
    const signed =
      secret !== undefined &&
      (await verifySignature(secret, request.headers.get(SIGNATURE_HEADER), raw, {
        toleranceSeconds: options.toleranceSeconds ?? DEFAULT_TOLERANCE_SECONDS,
      }));
    if (!signed) return failure(401, "invalid_signature", "the signature doesn't verify");

    let body: unknown;
    try {
      body = JSON.parse(new TextDecoder().decode(raw));
    } catch {
      return failure(400, "invalid_request", "the body isn't JSON");
    }
    const event = parseEnvelope(body);
    if (!event) return failure(400, "invalid_request", "the body isn't an event envelope");
    const context: WebhookContext = {
      deliveryId: request.headers.get(DELIVERY_ID_HEADER) ?? "",
      store,
      request,
    };
    const dedupe = context.deliveryId ? options.dedupe : undefined;
    try {
      if (dedupe && (await dedupe.seen(context.deliveryId, context)))
        return new Response(null, { status: 204 });
      await options.onEvent(event, context);
      await dedupe?.remember(context.deliveryId, context);
    } catch (error) {
      options.onError?.(error, event, context);
      return failure(500, "handler_failed", "the handler failed");
    }
    return new Response(null, { status: 204 });
  };
}

const ACTOR_TYPES: readonly WebhookActor["type"][] = ["user", "agent", "api_key", "app", "system"];

function actorType(value: unknown): WebhookActor["type"] | undefined {
  return ACTOR_TYPES.find((t) => t === value);
}

/** `body` as an envelope, when it has the envelope's shape. */
export function parseEnvelope(body: unknown): WebhookEnvelope | undefined {
  if (!isRecord(body)) return undefined;
  const { id, type, version, subject, actor, occurred_at, data } = body;
  if (
    typeof id !== "string" ||
    typeof type !== "string" ||
    typeof version !== "number" ||
    typeof occurred_at !== "string" ||
    !isRecord(subject) ||
    !isRecord(actor) ||
    !isRecord(data)
  )
    return undefined;
  const kind = actorType(actor.type);
  const actorId = actor.id;
  const subjectVersion = subject.version;
  if (
    typeof subject.type !== "string" ||
    typeof subject.id !== "string" ||
    !(subjectVersion === null || typeof subjectVersion === "number") ||
    kind === undefined ||
    !(actorId === null || typeof actorId === "string")
  )
    return undefined;
  return {
    id,
    type,
    version,
    subject: { type: subject.type, id: subject.id, version: subjectVersion },
    actor: { type: kind, id: actorId },
    occurred_at,
    data,
  };
}

function failure(status: number, code: string, error: string): Response {
  return Response.json({ code, error }, { status });
}
