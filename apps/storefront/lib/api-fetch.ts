// How server code reaches the API outside Workers (Node, `next build`): the global
// `fetch` to `HYPHA_API_URL`. On Workers, `api-fetch.workerd.ts` takes its place
// (package.json `imports`, the `workerd` condition).

/** The API's Service Binding as a `fetch`, when there is one; here never. */
export function apiFetch(): typeof fetch | undefined {
  return undefined;
}
