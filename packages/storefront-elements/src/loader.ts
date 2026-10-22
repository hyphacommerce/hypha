// Loading and mounting an app's storefront element: the app hosts an ES module that
// defines a custom element; `GET /storefront/app-extensions?target=…` lists `{ app, tag,
// module, props }`. Framework-free.

export interface ElementRef {
  /** The ES module's absolute URL. */
  module: string;
  /** The custom element's tag, e.g. `stripe-payment`. */
  tag: string;
}

/** What the loader needs of `customElements`. */
export interface ElementRegistry {
  get(tag: string): CustomElementConstructor | undefined;
  whenDefined(tag: string): Promise<CustomElementConstructor>;
}

export interface LoaderOptions {
  /** Imports a module by URL; a dynamic `import()` by default. */
  importModule?: (url: string) => Promise<unknown>;
  /** `customElements` by default. */
  registry?: ElementRegistry;
  /** How long a module may take to define its tag after loading. Default 10 s. */
  timeoutMs?: number;
}

export class ElementLoadError extends Error {
  readonly code: "invalid_tag" | "invalid_module" | "import_failed" | "not_defined";
  constructor(code: ElementLoadError["code"], message: string, options?: { cause?: unknown }) {
    super(message, options);
    this.name = "ElementLoadError";
    this.code = code;
  }
}

/** A valid custom element name: lowercase, starting with a letter, with a hyphen. */
const TAG = /^[a-z][a-z0-9._]*-[a-z0-9._-]*$/;

const LOCAL_HOSTS = new Set(["localhost", "127.0.0.1", "[::1]"]);

function moduleUrl(module: string): string {
  let url: URL;
  try {
    url = new URL(module);
  } catch {
    throw new ElementLoadError("invalid_module", `not an absolute URL: ${JSON.stringify(module)}`);
  }
  // `*.localhost` is this machine too (RFC 6761; browsers treat it as a secure origin):
  // `hypha deploy --target local` serves the API at `api.hypha.localhost` over http.
  const local =
    url.protocol === "http:" &&
    (LOCAL_HOSTS.has(url.hostname) || url.hostname.endsWith(".localhost"));
  if (url.protocol !== "https:" && !local)
    throw new ElementLoadError(
      "invalid_module",
      `elements load over https: ${JSON.stringify(module)}`,
    );
  return url.href;
}

const defaultImport = (url: string): Promise<unknown> =>
  import(/* @vite-ignore */ /* webpackIgnore: true */ url);

export function createElementLoader(options: LoaderOptions = {}) {
  const importModule = options.importModule ?? defaultImport;
  const timeoutMs = options.timeoutMs ?? 10_000;
  const imports = new Map<string, Promise<unknown>>();

  /**
   * Imports `module` once (concurrent and later calls share it; a failed import may be
   * retried) and resolves when `tag` is defined.
   */
  async function loadElement({ module, tag }: ElementRef): Promise<CustomElementConstructor> {
    if (!TAG.test(tag))
      throw new ElementLoadError(
        "invalid_tag",
        `not a custom element name: ${JSON.stringify(tag)}`,
      );
    const registry = options.registry ?? globalThis.customElements;
    const defined = registry.get(tag);
    if (defined) return defined;
    const url = moduleUrl(module);
    let loading = imports.get(url);
    if (!loading) {
      loading = importModule(url).catch((cause: unknown) => {
        imports.delete(url);
        throw new ElementLoadError("import_failed", `couldn't load ${url}`, { cause });
      });
      imports.set(url, loading);
    }
    await loading;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const timeout = new Promise<never>((_, reject) => {
      timer = setTimeout(
        () => reject(new ElementLoadError("not_defined", `${url} didn't define <${tag}>`)),
        timeoutMs,
      );
    });
    try {
      return await Promise.race([registry.whenDefined(tag), timeout]);
    } finally {
      clearTimeout(timer);
    }
  }

  return { loadElement };
}

/** The shared loader, on the page's `customElements`. */
export const { loadElement } = createElementLoader();

/** Sets `props` on `element` as properties (never attributes: they may be objects). */
export function setProps(element: Element, props: Record<string, unknown>): void {
  for (const [key, value] of Object.entries(props)) Reflect.set(element, key, value);
}

export interface MountOptions {
  tag: string;
  props?: Record<string, unknown>;
}

/**
 * Creates `<tag>` in `container`'s document, sets its props as properties and appends
 * it. The element upgrades when its module defines the tag (`loadElement`); props set
 * before that are kept by the element contract (docs/apps/elements.md).
 */
export function mountElement(container: Element, { tag, props = {} }: MountOptions): HTMLElement {
  if (!TAG.test(tag))
    throw new ElementLoadError("invalid_tag", `not a custom element name: ${JSON.stringify(tag)}`);
  const element = container.ownerDocument.createElement(tag);
  setProps(element, props);
  // `appendChild`, not `append`: Cloudflare's Workers types (in a storefront's project)
  // declare an HTMLRewriter `Element.append` that hides the DOM's.
  container.appendChild(element);
  return element;
}
