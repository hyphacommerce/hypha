// The app's side of the bridge: runs in the app's page inside the admin's iframe.
// Posts only to `hostOrigin` and accepts only messages from the parent window at
// exactly that origin.

import {
  type BridgeErrorCode,
  type Envelope,
  type Notifications,
  PROTOCOL_VERSION,
  type RequestType,
  type Requests,
  type ResourceType,
  type ToastTone,
  checkOrigin,
  idGenerator,
  isEnvelope,
  isRecord,
} from "./protocol.ts";

/** A request the host refused or didn't answer. */
export class BridgeError extends Error {
  readonly code: BridgeErrorCode;
  constructor(code: BridgeErrorCode, message: string) {
    super(message);
    this.name = "BridgeError";
    this.code = code;
  }
}

/** What the bridge needs from `window`; the real one by default. */
export interface BridgeWindow {
  parent: { postMessage(message: unknown, targetOrigin: string): void };
  addEventListener(type: "message", listener: (event: MessageEvent) => void): void;
  removeEventListener(type: "message", listener: (event: MessageEvent) => void): void;
}

/** Watches the page's height for `autoResize`; `ResizeObserver` on the document by default. */
export type HeightWatcher = (onHeight: (height: number) => void) => () => void;

export interface BridgeOptions {
  /** The admin's exact origin, e.g. `https://admin.shop.example`. Never `*`. */
  hostOrigin: string;
  /** Report the page's height whenever it changes. */
  autoResize?: boolean;
  /** How long to wait for an answer (not for the resource picker). Default 10 s. */
  timeoutMs?: number;
  window?: BridgeWindow;
  heightWatcher?: HeightWatcher;
}

export interface Bridge {
  /** A short-lived session token for the app's backend to verify. */
  getSessionToken(): Promise<string>;
  /** Asks the admin to go to `path` within the app's page (e.g. `/settings`). */
  navigate(path: string): Promise<void>;
  toast(message: string, options?: { tone?: ToastTone }): Promise<void>;
  /** The chosen resources' IDs; a `BridgeError` `cancelled` when nothing was chosen. */
  openResourcePicker(options: { type: ResourceType; multiple?: boolean }): Promise<string[]>;
  /** Tells the admin the page's height in CSS pixels. */
  resize(height: number): void;
  /** Asks the admin to close the page (a modal, a block's dialog). */
  close(): Promise<void>;
  /** Stops listening, rejects pending requests and stops auto-resize. */
  destroy(): void;
}

interface Pending {
  /** Takes the response's payload, as received. */
  settle(payload: unknown): void;
  reject(error: BridgeError): void;
  timer: ReturnType<typeof setTimeout> | undefined;
}

/** Reads each request's result, or `undefined` when it isn't what the protocol says. */
const RESULTS: { [T in RequestType]: (result: unknown) => Requests[T]["result"] | undefined } = {
  session_token: (result) =>
    isRecord(result) && typeof result.token === "string" ? { token: result.token } : undefined,
  resource_picker: (result) => {
    if (!isRecord(result) || !Array.isArray(result.selection)) return undefined;
    const ids: string[] = [];
    for (const item of result.selection as unknown[]) {
      if (!isRecord(item) || typeof item.id !== "string") return undefined;
      ids.push(item.id);
    }
    return { selection: ids.map((id) => ({ id })) };
  },
  navigate: () => null,
  toast: () => null,
  close: () => null,
};

const BRIDGE_ERROR_CODES: readonly string[] = [
  "unsupported",
  "cancelled",
  "invalid_request",
  "failed",
  "timeout",
] satisfies BridgeErrorCode[];

function isBridgeErrorCode(code: unknown): code is BridgeErrorCode {
  return typeof code === "string" && BRIDGE_ERROR_CODES.includes(code);
}

export function createBridge(options: BridgeOptions): Bridge {
  const hostOrigin = checkOrigin(options.hostOrigin, "hostOrigin");
  const win: BridgeWindow = options.window ?? globalThis.window;
  const timeoutMs = options.timeoutMs ?? 10_000;
  const nextId = idGenerator();
  const pending = new Map<string, Pending>();

  const post = (message: Envelope) => win.parent.postMessage(message, hostOrigin);

  const listener = (event: MessageEvent) => {
    if (event.origin !== hostOrigin || event.source !== win.parent) return;
    const data: unknown = event.data;
    if (!isEnvelope(data) || data.type !== "response") return;
    const waiting = pending.get(data.id);
    if (!waiting) return;
    pending.delete(data.id);
    clearTimeout(waiting.timer);
    waiting.settle(data.payload);
  };
  win.addEventListener("message", listener);

  function request<T extends RequestType>(
    type: T,
    payload: Requests[T]["payload"],
    wait: number | undefined = timeoutMs,
  ): Promise<Requests[T]["result"]> {
    const id = nextId();
    return new Promise((resolve, reject) => {
      const timer =
        wait === undefined
          ? undefined
          : setTimeout(() => {
              pending.delete(id);
              reject(new BridgeError("timeout", `no answer to ${type} in ${wait} ms`));
            }, wait);
      const settle = (answer: unknown) => {
        if (isRecord(answer) && answer.ok === true) {
          const result = RESULTS[type](answer.result);
          if (result !== undefined) return resolve(result);
        } else if (isRecord(answer) && answer.ok === false && isRecord(answer.error)) {
          const { code, message } = answer.error;
          if (isBridgeErrorCode(code))
            return reject(new BridgeError(code, typeof message === "string" ? message : code));
        }
        reject(new BridgeError("failed", `malformed response to ${type}`));
      };
      pending.set(id, { settle, reject, timer });
      post({ hypha: PROTOCOL_VERSION, id, type, payload });
    });
  }

  function notify<T extends keyof Notifications>(type: T, payload: Notifications[T]) {
    post({ hypha: PROTOCOL_VERSION, id: nextId(), type, payload });
  }

  const resize = (height: number) => {
    if (Number.isFinite(height) && height >= 0) notify("resize", { height: Math.ceil(height) });
  };

  let stopWatching: (() => void) | undefined;
  if (options.autoResize) stopWatching = (options.heightWatcher ?? observeDocumentHeight)(resize);

  notify("ready", { version: PROTOCOL_VERSION });

  return {
    getSessionToken: async () => (await request("session_token", {})).token,
    navigate: async (path) => {
      await request("navigate", { path });
    },
    toast: async (message, toastOptions) => {
      await request("toast", { message, tone: toastOptions?.tone ?? "info" });
    },
    openResourcePicker: async ({ type, multiple = false }) => {
      const { selection } = await request("resource_picker", { type, multiple }, undefined);
      return selection.map((item) => item.id);
    },
    resize,
    close: async () => {
      await request("close", {});
    },
    destroy: () => {
      win.removeEventListener("message", listener);
      stopWatching?.();
      for (const [id, waiting] of pending) {
        clearTimeout(waiting.timer);
        waiting.reject(new BridgeError("cancelled", "the bridge was destroyed"));
        pending.delete(id);
      }
    },
  };
}

/** The document's height, reported on every change through `ResizeObserver`. */
const observeDocumentHeight: HeightWatcher = (onHeight) => {
  const root = document.documentElement;
  const observer = new ResizeObserver(() => onHeight(root.scrollHeight));
  observer.observe(root);
  onHeight(root.scrollHeight);
  return () => observer.disconnect();
};
