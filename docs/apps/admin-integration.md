# App UI in a custom admin

Hypha's admin (`apps/admin`) shows what installed apps contribute: their pages, blocks
and actions (design note 0029, §5-§6), and it hosts the consent page apps are installed
through. A custom admin, in any framework, can do the same from the Admin API alone.
This page is what to build; `apps/admin` is the reference (`lib/app-extensions.ts`,
`components/app-extensions/`, `app/(admin)/apps/`).

Everything below goes through your admin's server with the staff member's session: the
session never reaches the browser's JavaScript or an app.

## 1. Discovery

`GET /admin/app-extensions` (optionally `?target=order.payment`) lists every installed
app's contributions the staff member may see, as flat lists:

| List | Each | Render it as |
| --- | --- | --- |
| `pages` | `{app, id, title, url, nav, target}` | an embedded page (§3), linked from the app's page (`apps.page`) or settings (`settings.app`) |
| `blocks` | `{app, id, target, url}` | a block (§2) where its target is |
| `actions` | `{app, id, label, target, input_schema, point}` | a button where its target is (§4) |
| `settings` | `{app, schema, secret_fields, proxy}` | a form from the JSON Schema, saved through the app proxy |
| `apps` | `{key, name, icon}` | labels |

Read it once per page view and pick the targets from it. A target your admin doesn't
implement is simply not shown; nothing breaks.

| Target | Where | Context to send |
| --- | --- | --- |
| `apps.page`, `settings.app` | full pages under the app | none |
| `order.details` | an order | `order_id` |
| `order.payment` | each payment of an order | `order_id`, `payment_id` |
| `order.fulfillment` | each fulfilment | `order_id`, `fulfillment_id` |
| `product.details` | a product | `product_id` |
| `customer.details` | a customer | `customer_id` |
| `payment` (actions) | a payment's buttons | `payment_id`, `order_id` |

## 2. Blocks

Fetch each block's tree from your server:
`GET /admin/app-extensions/blocks/{app}/{id}?order_id=…&payment_id=…` answers
`{app, id, target, tree}` with `tree` checked against the block format v1
(`packages/app-blocks/schema/block.v1.json`). Render it with your own components:
`@hypha/app-blocks/react`'s `<HyphaBlock tree onAction onNavigate>` does it on Astryx,
and any other design system needs a renderer for about fifteen elements.

- Fetch blocks in parallel and never let one break the page: a `502 app_unavailable`
  (the app failed, timed out, or answered something that isn't a v1 tree) is a small
  "the app didn't answer" note.
- Don't cache trees: they are the app's answer now.
- A block's `button` with `action` runs the app's action of that ID (§4) on the block's
  context, a `form`'s submit runs its `action` with the values as `input`; `navigate`
  opens the app's page at that path (§3).

## 3. Embedded pages and the bridge

Load the page's `url` in

```html
<iframe src="<url>?origin=<your admin's origin>"
        sandbox="allow-scripts allow-forms allow-same-origin allow-popups"></iframe>
```

and answer it with `@hypha/app-bridge-host` (protocol: [bridge.md](bridge.md)):

```ts
const detach = attachBridge(iframe, {
  appOrigin: new URL(iframe.src).origin,
  // POST /admin/app-extensions/{app}/session, through your server: { token }.
  getSessionToken: () => mySessionToken(appKey),
  navigate: (path) => router.push(`/apps/${appKey}/pages/${pageId}?path=${encodeURIComponent(path)}`),
  toast: (message, { tone }) => showToast(message, tone),
  pickResource: ({ type, multiple }) => openPicker(type, multiple), // IDs, or null
  onResize: (height) => { iframe.style.height = `${height}px`; },
  onClose: () => router.back(),
});
```

- `origin` is your admin's origin (`window.location.origin`); the app checks it against
  `GET /admin/app-extensions/origins` (`server.admin_url`) before talking to you.
- Session tokens are a minute long; fetch one per request the app makes.
- Handlers you don't provide answer `unsupported`, which apps must cope with (our admin
  picks products only).

## 4. Actions

- An action with a `point` (`payments.capture`, `.void`, `.refund`) runs through the
  payment's route: `POST /admin/payments/{payment_id}/capture | void | refund` (a refund
  takes `{amount}`, absent for all of it). 202 means the app accepted; the payment moves
  when the app reports back.
- Any other action is `POST /admin/app-extensions/actions/{app}/{id}` with
  `{context, input?}`, answered `{message, refresh, navigate}`: show `message` (plain
  text), reload what the action was on when `refresh`, open the app's page at `navigate`.
- An action with an `input_schema` asks first. Admin actions use flat objects: string,
  number/integer (with `minimum`/`maximum`), boolean and string `enum` properties, with
  `required`; our admin renders exactly those (`components/app-extensions/schema-form.ts`)
  with no JSON-Schema library, and leaves other properties to the app.

## 5. The consent page

Installing is OAuth (design note 0029, §4.1): the app sends the browser to Hypha's
`/oauth/authorize`, which redirects staff to **`{server.admin_url}/apps/consent?request=<id>`**.
That page is yours; it must:

1. Work signed out: log in first, then come back to the same URL (query included).
2. Read `GET /admin/oauth/requests/{id}`: the app (`name`, `icon`, `publisher`,
   `app_url`, `privacy_url`, `installed`, `private`), `groups` (non-personal scopes by
   area), `personal_data` (each `customer_data:*` scope with the app's `reason`: list
   these apart), `changes` (`added`, `removed`, `kept` against the current grant; show
   them when `installed`), `custom_fields` (below), `status` (`pending`, `approved`,
   `denied`, `expired`) and `expires_at`.
   - `custom_fields` has `added`, `kept`, `removed` and `retyped`, each a list of
     `{owner_type, key, name, type, personal}` against the manifest the app is installed
     from (everything is `added` on a first install). Say which owners the app adds
     fields to and flag the `personal` ones; list `removed` as "removes fields", since
     approving deletes them with their values; a non-empty `retyped` means the
     manifest can't be approved (`422 manifest_invalid`), so say why. See
     [custom-fields.md](custom-fields.md).
3. Explain when the staff member can't approve: approving needs `apps:write` and every
   scope requested (`403 not_allowed_to_grant` otherwise); denying needs `apps:write`.
4. Answer with `POST …/approve` or `…/deny` and then:
   - `redirect_to` set: navigate the browser there (a full navigation: it's the app's
     site, with `code` and `state`, or `error=access_denied`).
   - `credentials` set (a private app, below): show them once.
   - neither (a private app denied): go back to the apps list.
5. Show `409 oauth_request_expired` / `oauth_request_decided` as states, not errors.

### Installing from the admin

An app owns its OAuth flow (its PKCE verifier and redirect URI), so the admin doesn't
start it. Apps serve an install route, by convention

```
GET <app_url>/install?api=<the Hypha API's public base URL>
```

which starts `/oauth/authorize` against that API. Our admin's "Install app" asks for the
app's address (or its manifest URL, whose folder is taken) and opens that route
(`HYPHA_PUBLIC_API_URL`, else `HYPHA_API_URL`, is the `api`).

### Private apps

A store's own integration has no public site to redirect to. Upload its manifest:
`POST /admin/apps/private` with `{manifest}` (staff sessions only, `apps:write`; the
same rules as a served manifest, except `redirect_uris` may be empty). It answers
`{request, consent_url}`; send staff to the consent page. Approving answers
`credentials`: `{app, access_token, expires_in, refresh_token, token_endpoint, scopes,
app_secret}` (the secret only when the app is new). Show them once, like an API key; the
integration refreshes the pair at `token_endpoint` (`grant_type=refresh_token`) before
the access token's hour and the refresh token's 30 days run out. Uploading the manifest
again (new scopes, new UI) makes a new request; its approval issues a new pair and keeps
the secret.
