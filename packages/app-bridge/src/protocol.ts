// The bridge's wire protocol, version 1 (design note 0029, §6.3; docs/apps/bridge.md):
// what an embedded app page and the admin around it say over `postMessage`. Shared by
// `@hypha/app-bridge` (the app's side) and `@hypha/app-bridge-host` (the admin's).

export const PROTOCOL_VERSION = 1;

/** Every message, both ways. */
export interface Envelope<T extends string = string, P = unknown> {
  hypha: typeof PROTOCOL_VERSION;
  /** Unique per sender; a response carries its request's. */
  id: string;
  type: T;
  payload?: P;
}

export type ResourceType = "product" | "order" | "customer";
export const RESOURCE_TYPES: readonly ResourceType[] = ["product", "order", "customer"];

export type ToastTone = "info" | "success" | "warning" | "error";
export const TOAST_TONES: readonly ToastTone[] = ["info", "success", "warning", "error"];

/** The longest toast message hosts accept. */
export const MAX_TOAST_LENGTH = 500;

/** App → host, each answered by one `response`. */
export interface Requests {
  session_token: { payload: Record<string, never>; result: { token: string } };
  navigate: { payload: { path: string }; result: null };
  toast: { payload: { message: string; tone: ToastTone }; result: null };
  resource_picker: {
    payload: { type: ResourceType; multiple: boolean };
    result: { selection: { id: string }[] };
  };
  close: { payload: Record<string, never>; result: null };
}
export type RequestType = keyof Requests;
export const REQUEST_TYPES: readonly RequestType[] = [
  "session_token",
  "navigate",
  "toast",
  "resource_picker",
  "close",
];

/** App → host, never answered. */
export interface Notifications {
  /** Sent once when the bridge starts. */
  ready: { version: typeof PROTOCOL_VERSION };
  resize: { height: number };
}
export type NotificationType = keyof Notifications;
export const NOTIFICATION_TYPES: readonly NotificationType[] = ["ready", "resize"];

export type BridgeErrorCode =
  /** The host doesn't offer this (no handler). */
  | "unsupported"
  /** The person closed the picker (or another dialog) without choosing. */
  | "cancelled"
  /** The payload broke the protocol's rules. */
  | "invalid_request"
  /** The host's handler failed. */
  | "failed"
  /** No answer in time (raised by the app's side). */
  | "timeout";

export type ResponsePayload =
  | { ok: true; result: unknown }
  | { ok: false; error: { code: BridgeErrorCode; message: string } };

/** Host → app: the answer to the request with the same `id`. */
export type ResponseEnvelope = Envelope<"response", ResponsePayload>;

/** Whether `data` is a version-1 bridge message (any type). */
export function isEnvelope(data: unknown): data is Envelope {
  return (
    isRecord(data) &&
    data.hypha === PROTOCOL_VERSION &&
    typeof data.id === "string" &&
    data.id.length > 0 &&
    typeof data.type === "string"
  );
}

export function isToastTone(value: unknown): value is ToastTone {
  return typeof value === "string" && (TOAST_TONES as readonly string[]).includes(value);
}

export function isResourceType(value: unknown): value is ResourceType {
  return typeof value === "string" && (RESOURCE_TYPES as readonly string[]).includes(value);
}

/** Whether `value` is a plain object (what every payload is). */
export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/**
 * An exact origin (`https://admin.shop.example`): the only thing either side posts to or
 * accepts from. `*`, `null` and anything with a path are refused.
 */
export function checkOrigin(origin: string, role: string): string {
  let parsed: URL | undefined;
  try {
    parsed = new URL(origin);
  } catch {
    parsed = undefined;
  }
  if (!parsed || parsed.origin === "null" || parsed.origin !== origin)
    throw new TypeError(
      `${role} must be an exact origin like "https://admin.example.com", got ${JSON.stringify(origin)}`,
    );
  return origin;
}

/** IDs unique to this sender: a random prefix and a counter. */
export function idGenerator(): () => string {
  const random = crypto.getRandomValues(new Uint32Array(2));
  const prefix = Array.from(random, (n) => n.toString(36)).join("");
  let next = 0;
  return () => `${prefix}-${(next += 1)}`;
}
