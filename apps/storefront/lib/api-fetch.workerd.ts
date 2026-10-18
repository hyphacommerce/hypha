// On Workers (the `workerd` condition of package.json's `#api-fetch`): a deployment
// binds the API Worker as `API` (design note 0026, §2), so calls skip the internet, TLS
// and per-request fees, and the API knows them for ours (the binding's props). Without
// the binding (`vite dev`, `vite preview`), the global `fetch` to `HYPHA_API_URL`.

// Untyped on purpose: the Workers types are generated (`.cloudflare/types`), not
// committed, so a clean checkout's typecheck has no `cloudflare:workers`.
// @ts-ignore
import { env } from "cloudflare:workers";

type Binding = { fetch(input: RequestInfo | URL, init?: RequestInit): Promise<Response> };

/** The API's Service Binding as a `fetch`, when the deployment bound one. */
export function apiFetch(): typeof fetch | undefined {
  const binding = (env as { API?: Binding }).API;
  return binding ? (input, init) => binding.fetch(input, init) : undefined;
}
