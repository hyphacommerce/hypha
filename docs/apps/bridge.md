# The app bridge, protocol version 1

How an app's embedded page (design note 0029, §6.3) and the admin around it talk. The
page runs in an iframe; the two sides exchange `postMessage` messages. Either side can be
written without our packages: `@hypha/app-bridge` (inside the app) and
`@hypha/app-bridge-host` (in the admin) implement exactly what this page says.

## The iframe

The admin loads the app's page in

```html
<iframe src="https://app.example/hypha/admin"
        sandbox="allow-scripts allow-forms allow-same-origin allow-popups"></iframe>
```

The admin adds one query parameter to the page's URL, `origin`: its own origin
(`scheme://host[:port]`), so the page knows which `hostOrigin` to create its bridge
with:

```
https://app.example/hypha/admin?origin=https%3A%2F%2Fadmin.shop.example
```

The parameter is only a claim: anyone can open the page with any `origin`. Before
creating the bridge the page checks it against `GET /admin/app-extensions/origins` (the
store's admin origins; any admin caller may read it, so the app's backend reads it with
the installation's access token, and the page asks its backend) and refuses to start
when it isn't listed. Only then does it create the bridge and ask for a session token.

The app should send `Content-Security-Policy: frame-ancestors <the admin origins it
trusts>`; `GET /admin/app-extensions/origins` lists a store's admin origins.

## Envelope

Every message, both ways, is a plain object:

```json
{ "hypha": 1, "id": "k3x9a-7", "type": "toast", "payload": { "message": "Saved", "tone": "success" } }
```

| Field | Type | Meaning |
| --- | --- | --- |
| `hypha` | `1` | The protocol version. A message with any other value is ignored. |
| `id` | string, non-empty | Unique among the sender's messages. A response repeats its request's `id`. |
| `type` | string | What the message is (below). |
| `payload` | object | Depends on `type`. May be absent where the payload is empty. |

Anything that isn't such an object is ignored, so other scripts' messages on the same
window do no harm.

## App to admin: requests

Each request gets exactly one `response` with the same `id`.

| `type` | `payload` | `result` on success |
| --- | --- | --- |
| `session_token` | `{}` | `{ "token": "<JWT>" }` |
| `navigate` | `{ "path": "/settings" }` | `null` |
| `toast` | `{ "message": "Saved", "tone": "info" \| "success" \| "warning" \| "error" }` | `null` |
| `resource_picker` | `{ "type": "product" \| "order" \| "customer", "multiple": false }` | `{ "selection": [{ "id": "prod_…" }] }` |
| `close` | `{}` | `null` |

- `session_token`: the admin asks the API for a fresh token
  (`POST /admin/app-extensions/{app}/session`, through the admin's own server; the staff
  session never reaches the app). See *Session tokens* below.
- `navigate`: `path` must start with exactly one `/` and names a place within the app's
  page; the admin maps it under its own route for the app (our admin:
  `/apps/<key><path>`). Absolute and protocol-relative URLs are `invalid_request`.
- `toast`: `message` is 1 to 500 characters of plain text; `tone` defaults to `info`.
- `resource_picker`: the admin shows its own picker; closing it without choosing answers
  `cancelled`. `multiple` defaults to `false`.
- `close`: the admin closes whatever holds the page (a modal, a block's dialog); for a
  full page it may navigate back.

## App to admin: notifications

Never answered.

| `type` | `payload` | Meaning |
| --- | --- | --- |
| `ready` | `{ "version": 1 }` | The app's bridge started (sent once). |
| `resize` | `{ "height": 480 }` | The page's content height in CSS pixels (a non-negative number). The admin sizes the iframe to it. |

## Admin to app: responses

```json
{ "hypha": 1, "id": "<the request's id>", "type": "response", "payload": { "ok": true, "result": { "token": "…" } } }
{ "hypha": 1, "id": "<the request's id>", "type": "response", "payload": { "ok": false, "error": { "code": "cancelled", "message": "nothing was chosen" } } }
```

| `error.code` | When |
| --- | --- |
| `unsupported` | The admin doesn't offer this (an unknown `type`, or no handler for it). Apps must cope: a custom admin may implement none of the requests. |
| `cancelled` | The person dismissed the picker (or another dialog) without choosing. |
| `invalid_request` | The payload broke a rule above. |
| `failed` | The admin's handler failed. The message doesn't carry the admin's internal details. |
| `timeout` | Raised by the app's side when no response came in time (10 s by default; the picker waits as long as the person takes). Never sent by an admin. |

Codes are stable; messages are for humans.

## Security

- **Exact origins, both ways.** The app's bridge is created with the admin's origin
  (`createBridge({ hostOrigin: "https://admin.shop.example" })`), posts only with that
  `targetOrigin`, and accepts a message only when `event.origin` equals it **and**
  `event.source` is `window.parent`. The admin is attached with the app's origin
  (`attachBridge(iframe, { appOrigin: "https://app.example" })`), posts only to
  `iframe.contentWindow` with that `targetOrigin`, and accepts a message only when
  `event.origin` equals it **and** `event.source` is that iframe's window. `*` is never
  used and is refused as either origin, as are `null` and anything with a path.
- An origin is `scheme://host[:port]`, compared as a string: the app's origin is the
  origin of the iframe's `src`; the admin's is where the admin is served (the app learns
  it from the store's admin origins, or from the `origin` query parameter the admin adds
  to the page's URL, `?origin=https://admin.shop.example`, which the app checks against
  `GET /admin/app-extensions/origins` before trusting it). An admin always adds
  `origin`; after a `navigate` to `/settings`, our admin loads the page's URL with
  `/settings` appended to its path (`https://app.example/hypha/admin/settings?origin=…`).
- **What the app can't do:** read the admin's DOM, cookies or staff session (the sandbox
  and the origin boundary), choose where the admin navigates outside its own page, or
  put markup in a toast (plain text only).
- **What the admin must not trust:** a picked resource or a navigation path is a request,
  not an authorization: the app's backend acts through its own token and scopes.

## Session tokens

A JWT signed with HS256 and the app's secret (the one it got at install and verifies
webhooks with), valid for one minute:

| Claim | Value |
| --- | --- |
| `iss` | the Hypha API's base URL |
| `aud` | the app's key, e.g. `app_stripe` |
| `sub` | the staff member's user ID |
| `store` | the store's ID |
| `scope` | the app's scopes for this staff member, space-separated |
| `iat`, `exp` | issued at, expiry (`iat` + 60), Unix seconds |
| `jti` | a unique ID |

The app's page sends it to the app's backend (an `Authorization: Bearer` header), which
verifies it with `verifySessionToken(token, appSecret, { audience: "app_stripe" })` from
`@hypha/app`: the algorithm must be `HS256` (never `none`), the signature must match,
`exp` must be in the future and `aud` must be the app. Ask for a new token per call
rather than keeping one.

## Using the packages

Inside the app:

```ts
import { createBridge } from "@hypha/app-bridge";

const bridge = createBridge({ hostOrigin: adminOrigin, autoResize: true });
const token = await bridge.getSessionToken();
await fetch("/api/settings", { headers: { authorization: `Bearer ${token}` } });
await bridge.toast("Settings saved", { tone: "success" });
const [productId] = await bridge.openResourcePicker({ type: "product" });
```

In an admin:

```ts
import { attachBridge } from "@hypha/app-bridge-host";

const detach = attachBridge(iframe, {
  appOrigin: new URL(iframe.src).origin,
  getSessionToken: () => fetch(`/api/apps/${key}/session`, { method: "POST" }).then((r) => r.text()),
  navigate: (path) => router.push(`/apps/${key}${path}`),
  toast: (message, { tone }) => showToast(message, tone),
  pickResource: ({ type, multiple }) => openPicker(type, multiple), // IDs, or null if cancelled
  onResize: (height) => { iframe.style.height = `${height}px`; },
  onClose: () => closeModal(),
});
// later: detach();
```

## Versioning

A new request type or a new optional payload field is added within version 1 (unknown
types answer `unsupported`, so apps can feature-detect). A changed meaning or shape is
version 2, with a different `hypha` value; hosts will answer both while version 1 is
supported.
