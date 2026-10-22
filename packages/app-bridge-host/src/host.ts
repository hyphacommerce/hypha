// The admin's side of the bridge: answers the messages of one app page in one iframe.
// Accepts only messages whose source is that iframe's window and whose origin is exactly
// `appOrigin`, and posts only to that window at that origin.

import {
  type BridgeErrorCode,
  type Envelope,
  MAX_TOAST_LENGTH,
  PROTOCOL_VERSION,
  type RequestType,
  type Requests,
  type ResourceType,
  type ResponseEnvelope,
  type ToastTone,
  checkOrigin,
  isEnvelope,
  isRecord,
  isResourceType,
  isToastTone,
} from "@hypha/app-bridge/protocol";

/** What the host needs of the iframe: its window. */
export interface BridgeFrame {
  contentWindow: { postMessage(message: unknown, targetOrigin: string): void } | null;
  /** An iframe's document, whose window messages arrive on. */
  ownerDocument?: { defaultView: HostWindow | null } | null;
}

/** Where messages arrive: the admin's window. */
export interface HostWindow {
  addEventListener(type: "message", listener: (event: MessageEvent) => void): void;
  removeEventListener(type: "message", listener: (event: MessageEvent) => void): void;
}

/** A handler may throw this to answer a specific code (`cancelled`, …). */
export class BridgeHostError extends Error {
  readonly code: BridgeErrorCode;
  constructor(code: BridgeErrorCode, message: string) {
    super(message);
    this.name = "BridgeHostError";
    this.code = code;
  }
}

export interface AttachOptions {
  /** The app page's exact origin (from its URL). Never `*`. */
  appOrigin: string;
  /**
   * A fresh session token for the app (`POST /admin/app-extensions/{app}/session`,
   * through the admin's server: the staff session never reaches the browser's JS).
   */
  getSessionToken?: () => Promise<string>;
  /** Go to `path` within the app's page. */
  navigate?: (path: string) => void | Promise<void>;
  toast?: (message: string, options: { tone: ToastTone }) => void | Promise<void>;
  /** The chosen IDs, or `null` when the person cancelled. */
  pickResource?: (options: { type: ResourceType; multiple: boolean }) => Promise<string[] | null>;
  /** The page's height in CSS pixels. */
  onResize?: (height: number) => void;
  onClose?: () => void | Promise<void>;
  /** The app's bridge started. */
  onReady?: () => void;
  /** The window messages arrive on; the iframe's owner window by default. */
  window?: HostWindow;
}

type Handled = { [T in RequestType]: (payload: unknown) => Promise<Requests[T]["result"]> };

/** Starts answering `iframe`'s app; returns the function that stops it. */
export function attachBridge(iframe: BridgeFrame, options: AttachOptions): () => void {
  const appOrigin = checkOrigin(options.appOrigin, "appOrigin");
  const win: HostWindow = options.window ?? iframe.ownerDocument?.defaultView ?? globalThis.window;

  const handlers: Handled = {
    session_token: async () => {
      const get = need(options.getSessionToken, "session tokens");
      return { token: await get() };
    },
    navigate: async (payload) => {
      const { path } = record(payload);
      if (typeof path !== "string" || !path.startsWith("/") || path.startsWith("//"))
        throw invalid("navigate needs a path starting with one /");
      await need(options.navigate, "navigation")(path);
      return null;
    },
    toast: async (payload) => {
      const { message, tone } = record(payload);
      if (typeof message !== "string" || message.length === 0 || message.length > MAX_TOAST_LENGTH)
        throw invalid(`a toast needs a message of 1 to ${MAX_TOAST_LENGTH} characters`);
      const chosen = tone ?? "info";
      if (!isToastTone(chosen)) throw invalid(`unknown tone ${JSON.stringify(tone)}`);
      await need(options.toast, "toasts")(message, { tone: chosen });
      return null;
    },
    resource_picker: async (payload) => {
      const { type, multiple } = record(payload);
      if (!isResourceType(type)) throw invalid(`unknown resource type ${JSON.stringify(type)}`);
      if (multiple !== undefined && typeof multiple !== "boolean")
        throw invalid("multiple must be a boolean");
      const pick = need(options.pickResource, "resource pickers");
      const ids = await pick({ type, multiple: multiple ?? false });
      if (ids === null) throw new BridgeHostError("cancelled", "nothing was chosen");
      return { selection: ids.map((id) => ({ id })) };
    },
    close: async () => {
      await need(options.onClose, "closing")();
      return null;
    },
  };

  const respond = (id: string, payload: ResponseEnvelope["payload"]) => {
    const response: ResponseEnvelope = { hypha: PROTOCOL_VERSION, id, type: "response", payload };
    iframe.contentWindow?.postMessage(response, appOrigin);
  };

  const listener = (event: MessageEvent) => {
    if (
      event.origin !== appOrigin ||
      !iframe.contentWindow ||
      event.source !== iframe.contentWindow
    )
      return;
    const data: unknown = event.data;
    if (!isEnvelope(data)) return;
    const message: Envelope = data;
    if (message.type === "ready") {
      options.onReady?.();
      return;
    }
    if (message.type === "resize") {
      const { height } = record(message.payload);
      if (typeof height === "number" && Number.isFinite(height) && height >= 0)
        options.onResize?.(height);
      return;
    }
    const handler = (handlers as Record<string, (payload: unknown) => Promise<unknown>>)[
      message.type
    ];
    if (!handler) {
      respond(message.id, {
        ok: false,
        error: {
          code: "unsupported",
          message: `unknown message type ${JSON.stringify(message.type)}`,
        },
      });
      return;
    }
    handler(message.payload).then(
      (result) => respond(message.id, { ok: true, result }),
      (error: unknown) =>
        respond(message.id, {
          ok: false,
          error:
            error instanceof BridgeHostError
              ? { code: error.code, message: error.message }
              : // The host's own failure: its details stay in the admin.
                { code: "failed", message: `${message.type} failed` },
        }),
    );
  };

  win.addEventListener("message", listener);
  return () => win.removeEventListener("message", listener);
}

function need<T>(handler: T | undefined, what: string): T {
  if (!handler) throw new BridgeHostError("unsupported", `this admin offers no ${what}`);
  return handler;
}

function invalid(message: string): BridgeHostError {
  return new BridgeHostError("invalid_request", message);
}

function record(payload: unknown): Record<string, unknown> {
  return isRecord(payload) ? payload : {};
}
