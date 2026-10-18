// On Workers (the `workerd` condition of package.json's `#api-fetch`): a deployment
// binds the API Worker as `API` (design note 0026, §2), so calls skip the internet, TLS
// and per-request fees, and sends the deployment's caller key with each, so the API
// believes the client address we pass on (its `peer.rs`). Without the binding
// (`vite dev`, `vite preview`), the global `fetch` to `HYPHA_API_URL`.

// Untyped on purpose: the Workers types are generated (`.cloudflare/types`), not
// committed, so a clean checkout's typecheck has no `cloudflare:workers`.
// @ts-ignore
import { env } from "cloudflare:workers";

type Binding = { fetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> };

/** The API's Service Binding as a `fetch`, when the deployment bound one. */
export function apiFetch(): typeof fetch | undefined {
  const { API: binding, HYPHA_CALLER_KEY: key } = env as { API?: Binding; HYPHA_CALLER_KEY?: string };
  if (!binding) return undefined;
  return (input, init) => {
    const request = new Request(input, init);
    if (key) request.headers.set("x-hypha-caller", key);
    return binding.fetch(request);
  };
}
