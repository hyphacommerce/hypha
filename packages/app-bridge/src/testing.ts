// Test support: an admin window and an app window in its iframe, wired like a browser's
// `postMessage` (asynchronous, structured-cloned, delivered only when the target origin
// matches, with the sender's origin and window). Shared with @hypha/app-bridge-host's
// tests, and exported as `@hypha/app-bridge/testing` for apps testing their own pages.

type Listener = (event: MessageEvent) => void;

export class FakeWindow {
  readonly #listeners = new Set<Listener>();
  addEventListener(_type: "message", listener: Listener): void {
    this.#listeners.add(listener);
  }
  removeEventListener(_type: "message", listener: Listener): void {
    this.#listeners.delete(listener);
  }
  get listenerCount(): number {
    return this.#listeners.size;
  }
  deliver(data: unknown, origin: string, source: unknown): void {
    const event = { data: structuredClone(data), origin, source } as unknown as MessageEvent;
    for (const listener of this.#listeners) listener(event);
  }
}

export interface Posted {
  message: unknown;
  targetOrigin: string;
}

export function fakeFrame(adminOrigin: string, appOrigin: string) {
  const admin = new FakeWindow();
  const toApp: Posted[] = [];
  const toAdmin: Posted[] = [];
  // The app's view of its parent, and the admin's view of the iframe's window.
  const parent = {
    postMessage(message: unknown, targetOrigin: string) {
      toAdmin.push({ message, targetOrigin });
      if (targetOrigin === adminOrigin)
        queueMicrotask(() => admin.deliver(message, appOrigin, contentWindow));
    },
  };
  const contentWindow = {
    postMessage(message: unknown, targetOrigin: string) {
      toApp.push({ message, targetOrigin });
      if (targetOrigin === appOrigin)
        queueMicrotask(() => app.deliver(message, adminOrigin, parent));
    },
  };
  const app = Object.assign(new FakeWindow(), { parent });
  return { admin, app, iframe: { contentWindow }, parent, toApp, toAdmin };
}

/** Lets queued deliveries and the promises they settle run. */
export async function flush(): Promise<void> {
  for (let i = 0; i < 10; i++) await new Promise<void>((resolve) => setTimeout(resolve, 0));
}
