# 0035: Agents and MCP: preview, then apply

- **Status:** Proposed (2026-10-22). Danilo's answers are in §13; build order §14.
- **Scope:** admin agents as principals (`Actor::Agent`), what they may do and how a
  person stays in charge of it; **previewing any admin write** (the headline: an exact,
  rolled-back run of the real command); proposals staff approve; the MCP server that
  exposes the admin surface to agents (Claude, ChatGPT, Cursor, n8n, a merchant's own).
- **Not in scope:** agentic *shopping* (the UCP adapter, CLAUDE.md); running a model
  inside Hypha (the admin assistant, §12, comes later on this surface); event-triggered
  agents beyond webhooks (§12); developer tooling for building on Hypha (skills, §12);
  MCP Tasks and MCP Apps (§12).
- **Related:** 0008 (sessions, keys, scopes), 0010 (the two surfaces, OpenAPI, S9: error
  codes per operation "come with MCP generation", S10 `x-hypha-scopes`), 0016 (the
  outbox: what a write records), 0001 (one batch per write), 0026 (Workers, Hyperdrive),
  0029 (OAuth 2.1 with CIMD (AP5), personal-data scopes and redaction (AP10), app actions
  (§6.1: "MCP exposes actions as tools"), extension points (AP3)), 0030 (spores' routes).

## Why

Hypha was designed for this from the start: every operation is a command with typed
input, required scopes and typed errors, so "the same surface can be exposed over HTTP,
MCP and CLI" (CLAUDE.md). The admin API now covers the store (235 operations, 151 of
them writes), so the surface is worth exposing.

What nobody in the market has solved is **letting an agent write safely**. Every
platform that has shipped an admin MCP started read-only (Medusa, Saleor) or exposes
writes with no review step (commercetools); Shopify keeps writes inside Sidekick, where
app actions "stage changes for confirmation" on the app's own page. A merchant can't
let an agent fix 300 prices, or fulfil the morning's orders, without either trusting it
blindly or redoing the work by hand.

Hypha's commands make a better answer cheap. A write is one transaction that records
what it changed as contract events (0016). Running the real command, collecting those
events and the response, and rolling back gives an **exact preview** of any write, for
every operation, with no per-command code. The same mechanism carries a **proposal** to
a person who approves it, and an apply that **does exactly what was previewed or
nothing**. That's the headline: agents that can do real work in a store, with a person
seeing precisely what will change before it does.

## 1. What others do (October 2026)

- **Medusa** (announced 2026-10-20): a remote MCP server on Medusa Cloud only. Sign-in
  with the Cloud account, per-environment access. **Read-only** ("write operations
  coming soon"). Covers core and custom endpoints and workflows. Around it: scheduled
  agent tasks, Linear integration, a `feedback` tool. Not self-hostable.
- **Shopify:** no first-party admin MCP. Merchants get **Sidekick**, Shopify's own
  assistant; apps extend it with *data* extensions (searchable app content) and
  *action* extensions that open the app's page prefilled and stage changes for the
  merchant to confirm (open to all developers since June 2026). Shopping agents get the
  Storefront, Customer Account and Checkout MCPs, the Catalog API and UCP (with Google).
  Developers get the AI Toolkit (skills plus the CLI, sold as "highly token efficient")
  and Dev MCP. Admin MCPs for Shopify are community wrappers with full credentials.
- **commercetools** (managed MCP servers GA 2026-07-16): the most complete admin design.
  One server per agent, configured in the Merchant Center: tools per operation
  (`read_orders`, `create_orders`, …) chosen one by one or as `all`/`read_all`; response
  **field filters** (drop customer emails and addresses); **fixed parameters**;
  **per-project tool descriptions**; two auth modes, client credentials (autonomous) or
  the staff member's identity (tools ∩ their permissions); usage insights per agent. No
  review of writes.
- **Saleor:** an official read-only remote server. **Brink:** role-scoped actions in its
  merchant portal. **Stripe:** its own MCP for payment data.
- **The protocol** (2026-07-28 revision, the current one; checked against the
  specification on 2026-10-22, see "As built: hypha-mcp"): a stateless core (no
  initialize handshake and no sessions; version and client capabilities in every
  request's `_meta`, client info recommended, a mandatory `server/discover`), OAuth with
  **Client ID Metadata Documents** preferred over dynamic registration (now deprecated),
  Protected Resource Metadata (RFC 9728) and resource indicators (RFC 8707) required,
  Tasks and MCP Apps (`io.modelcontextprotocol/ui`) as extensions, elicitation through
  multi round-trip results (`input_required`) instead of server-sent requests.

What we take: commercetools' per-agent tool choice and its two modes (one agent per
person, one per job); Shopify's "stage changes for confirmation", made generic; Medusa's
breadth (custom routes are tools too: here, spores' and apps'). What we leave: hosted
only, read-only, one tool per endpoint in the model's context.

## Decisions

| | Decision |
| --- | --- |
| MC1 | **Preview is an API feature, not an MCP one.** Any admin write accepts `Hypha-Preview: 1`: the request runs for real in a transaction that's rolled back, with effects outside the database blocked, and answers `200` with a preview (§3) instead of the route's response. Staff, keys, apps and agents alike; the admin uses it too. *Hard to reverse (header, response shape)* |
| MC2 | **A preview shows what the write would return and what it would record**: the route's response body, the domain events (0016's contract payloads, already versioned), the emails it would queue or send, and the external calls it would make. A write that needs an external effect to continue (an app's `payments.refund`, a media upload) stops there and says so (`complete: false`, `blocked_by`); nothing is guessed |
| MC3 | **Apply does what was previewed or nothing.** A preview returns a `preview_token`; a write sent with `Hypha-Preview-Token` reuses the preview's generated IDs and is refused `409 preview_stale` (with the new preview in `details`) when the events it records differ from the preview's. *Hard to reverse (the guarantee clients build on)* |
| MC4 | **Agents are principals of their own**, `agent_…` (`AgentId` and `Actor::Agent` already exist). Two kinds: **connected** (a staff member connected an MCP client; it acts for them, its scopes capped at every request by their role, and it stops when they're deactivated) and **autonomous** (created by staff for a job, its own grant, no person behind it). *Hard to reverse* |
| MC5 | **Write scopes are granted with a mode: `propose` or `apply`.** Read scopes are plain. Every write scope is `propose` unless staff choose `apply`. Some scopes are never granted to an agent (§5.3). *Hard to reverse (the grant's shape)* |
| MC6 | **A proposal is a preview waiting for a person.** A write in `propose` mode is previewed and kept (`prop_…`) in the admin's inbox; a staff member holding its scopes approves it (applied under MC3) or rejects it; it expires after `agents.proposal_lifetime` (7 days). A stale one is previewed again on request |
| MC7 | **The MCP server is `/mcp` on the API**, both hosts, stateless Streamable HTTP. The protocol is its own crate, `hypha-mcp` (wasm-clean, knows nothing of routes or storage), behind an `Operations` port that `hypha-http` implements over the route table (§6). Auth is OAuth 2.1 through `hypha-oauth` with CIMD client IDs, as apps install (AP5); an agent's tokens are bound to `/mcp` (resource indicator) and refused elsewhere. *Hard to reverse (URL, auth flow)* |
| MC8 | **Tools are operations, found on demand.** A small fixed set (`find_operations`, `describe_operation`, `call`) reaches every admin operation the agent may use, by its OpenAPI `operationId` (`admin.products.update`), with schemas from the committed documents; a curated set of direct tools for the common jobs sits beside it, each named after its operation with `.` as `__` (`admin__products__update`), since model APIs refuse dots in tool names. Staff may narrow an agent to a list of operations. *Hard to reverse (tool names are a contract with agents' prompts)* |
| MC9 | **Personal data is off by default.** Tool results are redacted per the agent's grant through `redact.rs`, as for apps: names, emails, phones, addresses need `customer_data:*`, granted explicitly, connected agents included (a staff session sees everything; its agent doesn't). Every tool call is logged, reads too |
| MC10 | **Spores and apps extend the surface for free.** A spore's routes are operations; an app's admin actions (0029 §6.1) are operations, proposable, never previewed past their call |
| MC11 | **Hypha runs no model here.** Agents bring their own; the merchant chooses the provider and pays it. An in-admin assistant comes later on this same surface (§12) |
| MC12 | **The operator caps what any agent may write**: `agents.writes` in `hypha.config.toml`, `"apply"` (the default: grants decide), `"propose"` (no agent applies directly, whatever its grant says) or `"off"` (agents are read-only: write operations don't exist for them, previews and proposals included). The cap wins over every grant and isn't editable from the admin. *Hard to reverse (a key operators write)* |

## 2. The model

`hypha-agents`, a leaf domain (core only), beside `hypha-apps`:

- **`Agent`** (`agent_…`): name, kind (`connected { user: UserId }` /
  `autonomous`), the client that connected it (its CIMD client ID and name, for a
  connected one), `Grant` (§5), an optional operations allowlist (MC8), `enabled`,
  `last_used_at`, stamps, events (`agent.created`, `agent.updated`, `agent.revoked`).
- **`Proposal`** (`prop_…`): the agent, the operation ID, the request (path params,
  query, body: everything to run it again), the idempotency key, the preview (§3), its
  digest (§3.3), the scopes it needs, status `pending` / `applied` / `rejected` /
  `expired` / `failed`, who decided and when, the applied response. Events
  `proposal.created`, `proposal.decided` (so an app or Slack webhook can ping staff).
- **`AgentCall`** (`acall_…`): one row per tool call: agent, operation, mode (`read` /
  `preview` / `propose` / `apply`), outcome (status, error code), duration. No bodies
  (they may hold personal data). Pruned by `jobs.prune.agent_calls_after`, like
  `extension_calls`.

Credentials are `hypha-auth` token kinds stored as SHA-256 like every token: a connected
agent's OAuth pair (access 1 h, refresh 30 d, rotating, as apps'), an autonomous agent's
secret `hypha_ak_…` (shown once; used as a bearer at `/mcp` only).

The write's own events are stamped `Actor::Agent(id)` (the outbox already has
`actor_kind = 'agent'`). An approved proposal's write is the agent's too, with the
proposal's ID in the event's metadata: "who did it" is the agent, "who let it" the
proposal's decider. Idempotency keys are per actor already, so an agent's retries are
safe.

## 3. Previewing a write (MC1–MC3)

### 3.1 What the caller sends and gets

```http
PATCH /admin/products/prod_…/variants/var_…/price
Hypha-Preview: 1
Idempotency-Key: 6f1c…

{"amount": 2490}
```

```json
{
  "preview": {
    "complete": true,
    "response": {"status": 200, "body": {"id": "var_…", "price": {"amount": 2490, "currency": "EUR"}}},
    "events": [
      {"type": "product.variant_price_changed", "version": 1, "subject": "prod_…",
       "data": {"variant": "var_…", "from": 2990, "to": 2490}}
    ],
    "emails": [],
    "calls": [],
    "token": "hypha_pv_…",
    "expires_at": "2026-10-22T13:10:00Z"
  }
}
```

(The event shown is illustrative; each write records what its crate's `events.rs`
already says.)

- A preview of a write that fails answers that failure as the write would (`422`,
  `409`, …): an error is the most useful preview there is.
- `complete: false` with `blocked_by: {"kind": "extension_call", "point":
  "payments.refund", "app": "app_stripe"}` when the write reached an effect outside the
  database (§3.2). What it did up to there is shown, marked partial.
- The preview is redacted like the route's response would be for that caller (MC9);
  event payloads go through the same `Redaction` the webhooks use for apps.
- Reads ignore the header (`400 preview_not_a_write` would only annoy clients; a GET is
  its own preview).

### 3.2 How the host runs it

The request runs through the same handler, same command, same ports, with three
differences, all in the host and the adapter:

1. **One outer transaction, rolled back.** The request's `PgStore` is built in preview
   mode: it begins a transaction on checkout, and the adapter's three places that begin
   one (`in_transaction!`, `consistent_read!`, the batch commit in `batch.rs`) take a
   `SAVEPOINT` instead (and release or roll back to it); statements outside a
   transaction (`conn.db()`) run inside the outer one. Reads see the writes made before
   them, unique keys and versions are checked by Postgres as in a real write, and the
   events are read back from `outbox_events` before the `ROLLBACK`. A statement timeout
   (`agents.preview_timeout`, 5 s) bounds the locks a preview holds. On Workers the same
   adapter runs over Hyperdrive, whose connection stays pinned for a transaction.
   `consistent_read!`'s REPEATABLE READ snapshot can't nest: inside a preview, reads see
   the outer transaction's view, which is the point.
2. **Effects outside the database are recorded or blocked**, by swapping the host's
   capabilities for the request (`host.rs` is the one door, design-wise, so nothing can
   slip past it):
   - `HostMail`: a recorder; the preview lists template, recipient (redacted per MC9)
     and subject. Invites and resets are previewable without sending a token.
   - `HostCalls` (extension points, webhooks, manifests): **blocked**, except points
     declared read-only (`ExtensionPoint::READ_ONLY`: `shipping.rates`,
     `search.query`, `payments.status`), which are called for real and listed in
     `calls`. A blocked call fails the command with `PreviewBlocked`, which the host
     turns into `complete: false`.
   - `HostHttp` (media storage): blocked the same way.
   - `HostBackground`, `AfterWrites`: dropped; nothing was committed.
3. **IDs are recorded.** The request's `HostIds` notes every ID it hands out, in order;
   they go into the token (§3.3).

No command changes. A command that has an effect outside the database not routed
through `host.rs` would be a bug today already (it couldn't run on Workers); the route
tests gain a check that every admin write either previews completely or reports
`blocked_by` (§14, M1).

### 3.3 Applying exactly what was previewed (MC3)

The `token` (`hypha_pv_…`, stored hashed for `agents.preview_token_lifetime`, 15 min,
bound to the caller and the operation) holds the generated IDs and a **digest** of the
events: each event's type, version, subject and data, with values that are RFC 3339
timestamps blanked (times move between preview and apply; nothing else should).

Sending the same request with `Hypha-Preview-Token: hypha_pv_…`:

- hands the command the preview's IDs first (so the product created is the one shown),
- runs it for real, in its transaction,
- and before committing, compares the digest of the events it recorded: equal, it
  commits; different (someone changed the price meanwhile, stock moved), it rolls back
  and answers `409 preview_stale` with the fresh preview in `details`.

The check sits where the outbox rows are written, in the commit path, so it covers
single writes and batches alike. A token used twice is `409 preview_used` (the
idempotency key replays the first result instead, as today). A write with a blocked
preview (`complete: false`) can't use a token; it goes through a proposal, whose
approver sees what was and wasn't previewable.

## 4. Proposals (MC6)

- An agent with `propose` for a scope calls `call` with mode `apply` (or `propose`):
  Hypha previews, and stores a `Proposal` with the preview and its digest. The tool
  answers `{"proposed": "prop_…", "url": "<admin>/agents/proposals/prop_…"}`. A failing
  preview isn't proposed: the agent gets the error, as it would on a direct write.
- **The inbox** (`/agents/proposals` in the admin, and an item on the home page's "what
  needs doing"): each proposal with its agent, the person it acts for, the operation's
  summary, the events rendered as a change list (event types get a short human template
  in the admin, falling back to the JSON), the emails it would send, and the record as it
  is now beside what it would be.
- **Approve** (`POST /admin/agent-proposals/{id}/approve`, a staff session holding every
  scope the proposal needs): applies with the stored token semantics (§3.3), re-previewed
  first if the token expired; stale is shown, not applied, with "preview again".
  **Reject** takes an optional note the agent can read (`get_proposal`).
- **Bulk**: proposals from one agent call batch can be approved together; each is its
  own write (no cross-proposal transaction in v1).
- Expired after `agents.proposal_lifetime`; the agent can read every outcome, so a
  scheduled agent learns what was approved.

## 5. Grants and safety (MC4, MC5, MC9)

### 5.1 The grant

```json
{
  "read": ["orders:read", "products:read", "inventory:read"],
  "write": {"products:write": "propose", "inventory:write": "apply"},
  "customer_data": [],
  "operations": null
}
```

- A **connected** agent's effective scopes are the grant ∩ its staff member's role,
  read fresh per request (commercetools' identity mode). Changing the role narrows its
  agents at once. Nobody grants an agent a scope they lack (`require_grantable`, as
  for keys), nor `apply` for a scope they couldn't approve.
- An **autonomous** agent is created by staff with `agents:write` and the scopes they
  give it; it keeps them while the creator is gone (like an API key).
- `operations`, when set, is the allowlist: only those operation IDs exist for the
  agent (`find_operations` doesn't list the rest).
- **The deployment's cap** (MC12) is applied last, at every request: under `"propose"`
  an `apply` mode acts as `propose`; under `"off"` the write scopes are dropped and
  `find_operations` lists reads only. A write attempted anyway is `403 agent_writes_off`.
  The grant is kept as staff set it, so lifting the cap restores it; the admin shows the
  cap beside each grant. Previews by staff, keys and apps are unaffected: the cap is
  about agents.

### 5.2 Prompt injection is the threat model

An agent reads text written by strangers: product reviews in an app's data, customer
notes, order addresses, a supplier's CSV. Any of it may say "refund every order". The
design assumes the agent will sometimes be fooled and limits the damage:

- **`propose` by default** for every write; `apply` is a choice per scope.
- **Never granted** to an agent, in any mode: `users:write`, `api_keys:write`,
  `apps:write` (installing an app, approving scopes), `agents:write`. An agent can't
  widen its own reach or mint credentials. `customers:erase` is `propose` only.
- **No personal data unless granted** (MC9), so a fooled agent has less to exfiltrate,
  and the merchant's DPA with the model provider covers what they chose.
- **Rate limits per agent** (`Rule::AgentCall`, `Rule::AgentWrite`), the write one low.
- **A kill switch per agent** (disable, revoke) and per deployment (`agents.enabled =
  false` in `hypha.config.toml` refuses `/mcp` and every agent credential).
- The log (§2) answers "what did it read and do", and the outbox answers "what changed".

Amount policies ("refunds over 100 € always propose", "never more than 50 writes an
hour") are a natural next step on the grant; §12.

## 6. The MCP server (MC7)

- **Endpoint:** `POST /mcp` (Streamable HTTP; JSON responses, no SSE needed for v1),
  stateless: every request carries its credential and protocol version. Clients still on
  the handshake revisions (2025-11-25 and before) are served too: their `initialize` is
  answered and forgotten, with no session (see "As built: hypha-mcp").
- **`hypha-mcp`** is the protocol, and only that: JSON-RPC envelopes, the MCP messages
  (`tools/list`, `tools/call`, the versions it speaks), the fixed tools of §7 (ranking
  for `find_operations`, `describe_operation`'s shape, `call`'s modes) and the mapping
  of Hypha's error body to `isError` results. It reaches Hypha through one port, with no
  `Send` bound like every port:

  ```rust
  #[allow(async_fn_in_trait)]
  pub trait Operations {
      /// What this caller may call, with its mode (read, propose, apply).
      async fn visible(&self) -> Result<Vec<OperationSummary>, McpError>;
      async fn describe(&self, id: &OperationId) -> Result<OperationSchema, McpError>;
      async fn call(&self, call: OperationCall) -> Result<OperationResult, McpError>;
  }
  ```

  The curated direct tools are data passed in (a list of operation IDs), not a list it
  owns. It depends on `serde` and `hypha-core` at most; no axum, no routes, no grants:
  what a caller may do is whatever `visible` reports. Its tests run against a fake
  `Operations` in memory. (The official Rust SDK, `rmcp`, pulls Tokio; we take its
  types only if they build for wasm, else write the few we need.)
- **`hypha-http`** implements `Operations` per request: `visible` and `describe` from
  the route table and the committed OpenAPI documents (filtered by the caller's
  effective scopes, allowlist and MC12's cap), `call` as an in-process request through
  the concrete router (the host's router macro mounts `/mcp` with a clone of it). So a
  tool call goes through the same handlers: same auth, redaction, errors, idempotency,
  preview. No second implementation of any operation. Like `hypha-extensions`, only
  hosts' layer (`hypha-http`) depends on it.
- **Discovery:** `/.well-known/oauth-protected-resource` (RFC 9728) names `/mcp` and the
  authorization server; `/.well-known/oauth-authorization-server` exists already.
- **Connecting a client** (a connected agent): the MCP client starts OAuth with its CIMD
  client ID (Claude's, ChatGPT's, any client's metadata URL, fetched over `HostCalls`
  like a manifest) and `resource=<api>/mcp`; `/oauth/authorize` stores the request and
  sends the staff member to the admin's `/agents/connect?request=oareq_…`: the client's
  name and site, the scopes it asks for, and the grant they'll give (preset to read-only
  with writes as `propose`, no customer data). Approving creates the agent and issues
  the code. `hypha-oauth` gains a client kind (app or agent); codes, PKCE and rotation
  are unchanged.
- **Autonomous agents** connect with their `hypha_ak_…` as a bearer token (an MCP client
  configured with a header), for scripts, n8n, a scheduled Claude routine.

## 7. Tools (MC8)

Dumping 235 tool definitions into a model's context costs tokens on every turn and
makes it worse at picking one (Shopify's reason for skills over Dev MCP). So:

**The fixed set**, always present:

| Tool | Does |
| --- | --- |
| `find_operations` | words → operations the agent may call: `operationId`, summary, read/write, its mode (`read`/`propose`/`apply`) |
| `describe_operation` | an `operationId` → its input schema (path, query, body, from the OpenAPI document), what it returns, its error codes, its scope |
| `call` | `{operation, params, query, body, mode}`: `mode` is `preview`, `apply` (becomes a proposal under `propose`, §4) or `propose` |
| `get_proposal` / `list_proposals` | the agent's own proposals and their outcomes |
| `whoami` | the agent, its grant, the store, its currency and time zone |

**Direct tools**, curated in a list in `hypha-http` and passed to `hypha-mcp`: the
daily work of running a store (what's come in, what to ship, what's running out, what
to change in the catalog), named after their operations (dots as `__`, below), and
`find_operations` names a curated operation's tool beside its ID, so an agent that
found one and one listed directly call the same thing. Writes among them
take the same `mode`. Twenty-three, 14 reads and 9 writes:

| Job | Reads | Writes |
| --- | --- | --- |
| Orientation | `admin.setup.checklist` | |
| Orders | `admin.orders.list`, `admin.orders.fetch`, `admin.fulfillments.list` | `admin.fulfillments.create`, `admin.orders.mark_paid`, `admin.orders.cancel` |
| Customers | `admin.customers.list`, `admin.customers.fetch` | |
| Catalog | `admin.products.list`, `admin.products.fetch`, `admin.categories.list` | `admin.products.update`, `admin.products.update_variant`, `admin.products.set_status`, `admin.categories.add_products` |
| Stock | `admin.inventory.list_items` (`below`: what's running low), `admin.inventory.fetch_product_inventory`, `admin.inventory.list_locations` | `admin.inventory.adjust_level` |
| Prices | `admin.markets.list_markets`, `admin.markets.list_fixed_prices` | `admin.markets.set_fixed_prices` |

Left to `find_operations` on purpose: refunds, captures and voids (money leaving;
proposable, rarely what a routine needs), customer writes and erasure, settings
(taxes, shipping, payment methods, notifications), and everything about apps.

Two gaps in the API show up here, and every competitor's examples ask for them ("which
products are running low", "how did last week sell"): there's no list of inventory
items across products with a `below` filter, and no sales summary. Both are ordinary
routes (§14, M6), useful to the admin's home page too.

- A direct tool's name is its OpenAPI `operationId`, which the route table already
  makes stable, with each `.` as `__` (`admin__orders__list`): model APIs refuse dots in
  tool names (see "As built: hypha-mcp"). `call` and the other fixed tools keep the
  dotted ID. Renaming an operation is a breaking change, like an error code.
- Descriptions come from the operation's documentation; the error codes an operation
  can return (0010 S9, still in prose today) become data here: each route lists its
  command's `ErrorCode`s in an `x-hypha-errors` extension, which `describe_operation`
  returns. This is the one piece of per-route work the note asks for.
- Errors are tool results with `isError: true` and Hypha's body (`{code, error,
  details}`): the agent sees `insufficient_stock` with the shortage, not a 409.
- Spores' routes are in the route table, so they're operations (MC10). An installed
  app's admin actions appear as `app.<key>.<action>` with the action's input schema;
  calling one is an external call, so it can be proposed, never fully previewed.

## 8. What the admin gets

- **Settings → Agents** (Manage): connected and autonomous agents, their grants (scope
  list with a mode switch per write scope, customer data, the operations allowlist),
  last used, activity (the call log and the events stamped with the agent), disable,
  revoke. "Create agent" for an autonomous one shows its key once.
- **The consent page** for connecting a client (§6).
- **The proposals inbox** and the home page item (§4).
- **Preview in the admin's own forms** where it helps (bulk price edits, a fulfilment
  that splits stock): the same header, so staff get the same safety the agent does.
  Not required for v1.

## 9. Routes and errors

| Route | Scope |
| --- | --- |
| `GET /admin/agents`, `GET /admin/agents/{id}` | `agents:read` |
| `POST /admin/agents` (autonomous), `PATCH /admin/agents/{id}`, `POST …/{id}/revoke` | `agents:write` |
| `GET /admin/agents/{id}/calls` | `agents:read` |
| `GET /admin/agent-proposals` (the ones whose scopes the caller holds), `GET …/{id}` | `agents:read` |
| `POST /admin/agent-proposals/{id}/approve`, `…/reject` | a staff session with the proposal's scopes |
| `POST /mcp` | an agent's credential |

New codes: `preview_stale` (409, `details.preview`), `preview_used` (409),
`preview_token_invalid` (422: expired, another caller's, another operation),
`preview_blocked` (only inside a proposal's record), `proposal_not_found` (404),
`proposal_decided` (409), `proposal_expired` (409), `agent_disabled` (403),
`agent_scope_not_grantable` (422), `operation_not_allowed` (403: outside the allowlist),
`agents_disabled` (403), `agent_writes_off` (403: MC12).

New scopes: `agents:read`, `agents:write` (owner by default).

## 10. Configuration

`[agents]` in `hypha.config.toml`: `enabled` (true), `writes` (`"apply"` / `"propose"` /
`"off"`, MC12; `HYPHA_AGENT_WRITES` too, for a deployment set up without a file),
`proposal_lifetime` (7 d),
`preview_token_lifetime` (15 min), `preview_timeout` (5 s); `jobs.prune.agent_calls_after`
(30 d); the rate rules' quotas in `[rate_limits]`.

## 11. Hard to reverse

- The preview contract: `Hypha-Preview`, `Hypha-Preview-Token`, the preview's shape,
  `complete`/`blocked_by` (MC1–MC3).
- What "apply what was previewed" compares: the event digest (§3.3). Comparing less
  later would weaken a promise; comparing more would refuse writes that pass today.
- `Actor::Agent` and the two kinds; a connected agent capped by its person (MC4).
- The grant's shape with modes, and the scopes never granted (MC5, §5.2).
- `/mcp`, its auth (CIMD, resource-bound tokens), and direct tool names = operation IDs
  with `.` as `__` (MC7, MC8).
- `agents.writes` and its three values (MC12).

## 12. Later

- **MCP Tasks** for long work (a bulk import, re-pricing a catalog): the call returns a
  task, proposals land as they're ready.
- **MCP Apps**: render a proposal's change list, and app blocks (0029 §6.2), inside the
  client.
- **Policies on grants**: amount caps, write budgets per hour, "always propose for
  orders over X", time windows.
- **The admin assistant**: a chat in the admin, as a spore behind a `ModelProvider` port
  (the merchant's key and provider), using this surface as a connected agent of the
  signed-in staff member. Its tool calls are the ones in §7; nothing new in core.
- **Triggers**: today an agent reacts to events through webhooks it receives itself;
  later, an autonomous agent subscribes to events like an app, and Hypha can call a
  hosted agent runner.
- **Developer skills**: Claude Code / Codex skills for writing spores and apps against
  `docs/spores` and `@hypha/app`, and an agent-friendly `hypha` CLI (it already has a
  flag for every prompt).
- **Storefront agents** stay on UCP, never `/mcp`.

## 13. Danilo's answers (2026-10-22)

1. **Preview for every caller:** yes (MC1). Staff, keys, apps and agents.
2. **Autonomous agents in v1:** yes, both kinds (MC4).
3. **Default grant read + `propose`:** yes, plus a deployment-wide cap so an operator
   can make agents read-only, or never let them apply directly (MC12, `agents.writes`).
4. **The never-granted scopes are fixed in code** (§5.2), not a setting.
5. **Direct tools:** the 22 in §7, proposed here; open to changes before M6.
6. **The protocol is its own crate, `hypha-mcp`**, behind an `Operations` port that
   `hypha-http` implements (§6), rather than living inside `hypha-http`; agents,
   grants and proposals are the leaf `hypha-agents`.

## 14. Build order

1. **M1: preview in the adapter and host.** The preview-mode `PgStore` (savepoints, outer
   rollback, timeout), the recording and blocking capabilities, recorded IDs, the
   `Hypha-Preview` header for admin writes. A route test that previews every admin write
   from the route tests' fixtures and checks nothing persisted, and that each is complete
   or names its block.
2. **M2: tokens and apply-what-was-previewed.** Digest at the outbox write, the token,
   `preview_stale`, `preview_used`.
3. **M3: `hypha-agents`.** Agents, grants with modes, the never-granted rule, `AgentCall`,
   repos, the contract, Postgres; `agents:*` scopes; redaction for `Actor::Agent`.
4. **M4: proposals.** The aggregate, propose-on-apply, approve/reject routes, events.
5. **M5: `hypha-mcp` and `/mcp`.** The crate (JSON-RPC, the fixed tools, the
   `Operations` port, tests on a fake), `hypha-http`'s implementation, protected
   resource metadata, the agent client kind in `hypha-oauth`, autonomous keys, rate
   rules, `agents.writes`; `x-hypha-errors`.
6. **M6: direct tools, app actions and the two missing reads.** The curated list;
   `app.<key>.<action>`; inventory items across products with `below`, a sales
   summary.
7. **M7: admin.** Agents settings, the consent page, the inbox, the home page item.
8. **M8: e2e and docs.** `scripts/e2e.sh` steps (connect, preview, propose, approve,
   stale), `docs/agents/` (connecting Claude, ChatGPT, Cursor, n8n), the Workers check.

Sources for §1: [Medusa MCP](https://medusajs.com/blog/announcing-medusa-mcp),
[Shopify Spring '26 (dev)](https://www.shopify.com/news/spring-26-edition-dev),
[Sidekick app extensions](https://shopify.dev/changelog/sidekick-app-extensions-available-today),
[commercetools managed MCP servers](https://docs.commercetools.com/api/managed-mcp-servers-overview),
[Saleor MCP](https://glama.ai/mcp/servers/@saleor/saleor-mcp),
[MCP 2026-07-28 (WorkOS)](https://workos.com/blog/mcp-2026-spec-agent-authentication).

## As built: preview (M1)

- **Where a preview starts.** `crate::preview::middleware` (layered by both hosts over every
  route, spores' included) takes any non-GET under `/admin/` with `Hypha-Preview: 1`; `0`
  or no header runs the write, anything else is `400 invalid_preview_header`. Every set of
  ports such a request opens goes through `AppState::public_ports`, which begins the
  preview on them (`Storage::begin_preview`), so `Caller::authenticate`, `StaffCaller` and
  the credential-free routes are all covered. `Storage::directory()` (a model C control
  plane) isn't; both hosts serve the directory from the request's ports.
- **The adapter.** `PgStore::begin_preview` runs `BEGIN` with `SET LOCAL statement_timeout`
  (`agents.preview_timeout`, 100 ms to 30 s, 5 s) and `idle_in_transaction_session_timeout`
  (`PREVIEW_LEFT_OPEN`, 60 s). The three places that began a transaction now begin a `Tx`,
  which in a preview is a savepoint, released or rolled back to; `consistent_read!` reads
  the preview's own view in a savepoint. Different from §3.2: a statement *outside* a write
  (`conn.db()`) takes a savepoint of its own too, or one failed statement (a reference on a
  delete) would abort the whole transaction and the next read would answer 500 instead of
  the write's own error. A store whose preview ended refuses further statements.
- **Events: read back, by the rows written.** `insert_events` returns each row's `seq`;
  in a preview they're noted, and `PgPreview::end` reads `outbox_events WHERE seq = ANY(…)`
  before the `ROLLBACK`. A write rolled back to its savepoint (a lost race retried, a
  duplicate) takes its rows with it, so what's read back is exactly what would have
  committed, in order, as stored (the contract form M2 digests). Capturing payloads at the
  write would have to undo captures on every savepoint rollback; reading the table without
  the `seq`s can't tell our rows from other transactions' (rows from subtransactions don't
  carry the top transaction's xid).
- **Leaks.** The open preview shares the store's connection (`Arc<Mutex<C>>`), so the
  middleware ends it after the handler dropped its ports. A request dropped or panicking
  before that leaves the connection marked (`PgConnection::preview_open`): the native pool
  rolls it back in `recycle` before handing it out (deadpool's `Fast` recycling only checks
  the socket), and Postgres ends a transaction idle for 60 s, which bounds the locks it
  holds meanwhile; a Workers connection closes when dropped.
- **Capabilities.** Not a swapped `AppState` (it's shared, and copying it per request means
  copying the storage): the request's `Preview` is current while its future is polled
  (`InPreview`, a thread-local set around each poll, as Workers' `Scoped`), and each door in
  `host.rs` asks for it. `HostMail` records (template, recipient, subject; `OutgoingEmail`
  gained `template`) and answers sent; `HostCalls` sends only calls marked
  `CallPurpose::Extension { read_only: true }` (`ExtensionPoint::READ_ONLY`, now required on
  every point; `OutgoingCall` carries the point, `READ_ONLY` and the app) and lists them in
  `calls`; `HostHttp` is blocked (`media_storage`); presigning an upload needs no HTTP, so
  `HostBlobs` (now a newtype) blocks it (`media_upload`), or the preview would hand out a
  working upload URL; `HostBackground::spawn` drops the work (and says it was taken),
  `AfterWrites` and `MailQueued` stay quiet, and the store never calls its dispatch hook.
  `blocked_by.kind` is `extension_call` (with `point`, `app`), `outbound_call` (webhooks,
  manifests, spores' HTTP), `media_storage` or `media_upload`; the first blocked effect is
  the one reported, and `response` is `null` when the write failed there.
- **Emails** are the ones sent while answering (invites, test sends). Emails queued later by
  the `Mailer` from the write's events (an order's) aren't listed yet: listing them means
  running the `Mailer` over the preview's events inside its transaction, before the
  rollback. A password reset runs in the background and so previews as nothing. Recipients
  show for staff sessions and `customer_data:email`, else `to` is absent and listed in
  `redacted`.
- **Redaction.** `response.body` is the handler's own, already redacted. Events go through
  `redact::event_data` for anyone but a staff session, and `*.custom_fields_updated`
  entries through `changes_visible_to` with the definitions read when the caller
  authenticated (before the write), as `GET /admin/events/{id}` shows them.
- **IDs** handed out by `HostIds` are noted in order and put on the preview's response as
  the `PreviewIds` extension, not in its body: M2 puts them in the token.
- **OpenAPI.** Every admin write has `#/components/parameters/HyphaPreview` and
  `x-hypha-preview: {"$ref": "#/components/responses/Preview"}` (schema `PreviewResponse`),
  plus `invalid_preview_header` among its 400s. Not a second `200`: a write that answers
  `200` itself can't have two, and its own responses keep the SDK's types exact.
- **Settings.** `agents.preview_timeout` is `hypha_config::AgentsConfig`: no crate below
  `hypha-config` owns it (the adapter and `hypha-http` use it); M3 can grow `[agents]`.
- **Not yet:** the route test that previews *every* admin write from the fixtures (§14 M1);
  the tests cover a product create, a variant change, a stock adjustment, a keyed
  fulfilment followed by the real one, a cancellation, an invite (and its redaction), an
  app-backed capture (blocked) and failures.

## As built: hypha-mcp

`crates/hypha-mcp` (M5's protocol half), serde and `hypha-core` only, builds for wasm.

**What the specification says** (read 2026-10-22). 2026-07-28 is the current revision
([versioning](https://modelcontextprotocol.io/specification/versioning)); 2025-11-25 the
previous, last of the handshake era.

- *Stateless* ([changelog](https://modelcontextprotocol.io/specification/2026-07-28/changelog),
  [basic](https://modelcontextprotocol.io/specification/2026-07-28/basic/index)): no
  `initialize`, no sessions, no `ping`. Every request's `params._meta` carries
  `io.modelcontextprotocol/protocolVersion` and `…/clientCapabilities` (required: missing
  is `-32602`, HTTP 400) and should carry `…/clientInfo`; results carry `resultType:
  "complete"` and should carry `_meta["io.modelcontextprotocol/serverInfo"]`. Servers
  **must** answer `server/discover`
  ([discover](https://modelcontextprotocol.io/specification/2026-07-28/server/discover)).
- *Versions* ([versioning](https://modelcontextprotocol.io/specification/2026-07-28/basic/versioning)):
  an unsupported one is `-32022` with `data: {supported, requested}`. A dual-era server
  serves a request with modern `_meta` statelessly and an `initialize` under the legacy
  revision. Legacy negotiation ([2025-11-25 lifecycle](https://modelcontextprotocol.io/specification/2025-11-25/basic/lifecycle)):
  answer the client's version when supported, else our latest.
- *Streamable HTTP* ([2026-07-28](https://modelcontextprotocol.io/specification/2026-07-28/basic/transports/streamable-http),
  [2025-11-25](https://modelcontextprotocol.io/specification/2025-11-25/basic/transports)):
  POST only (GET and DELETE `405`; ignore `Mcp-Session-Id` and `Last-Event-ID`), one
  message per POST (no batches), a notification answered `202` with no body, a request
  with `application/json` or SSE. Modern requests mirror the body in headers:
  `MCP-Protocol-Version` (must equal `_meta`'s), `Mcp-Method` on every request, `Mcp-Name`
  on `tools/call` (`=?base64?…?=` for unsafe values); missing or different is `400` with
  `-32020 HeaderMismatch`. An unknown method is `404` with `-32601`. Without the version
  header a server may assume 2025-03-26. `Origin` must be checked (`403`).
- *Tools* ([tools](https://modelcontextprotocol.io/specification/2026-07-28/server/tools)):
  names should be 1–128 of `A-Z a-z 0-9 _ - .` (`admin.tools.list` is the spec's own
  example), but model APIs are stricter, so direct tools' names are mapped (below). The list may vary
  by authorization, in a deterministic order; `tools/list` results must carry `ttlMs` and
  `cacheScope` ([caching](https://modelcontextprotocol.io/specification/2026-07-28/server/utilities/caching):
  `private` when it varies per caller). Pagination is optional; an invalid cursor is
  `-32602` ([pagination](https://modelcontextprotocol.io/specification/2026-07-28/server/utilities/pagination)).
  An unknown tool or a malformed call is a protocol error (`-32602`); input validation
  and business errors are results with `isError: true`. `structuredContent` should be
  mirrored as text; 2026-07-28 allows any JSON value there, 2025-11-25 only an object.

**What was built.** `Server::handle(&impl Operations, McpRequest {body, headers}) ->
McpResponse {status, body}`; transport concerns (auth, `Origin`, `405`) stay in
`hypha-http`. Modern: `server/discover`, `tools/list`, `tools/call`. Legacy (2025-11-25,
2025-06-18, 2025-03-26, by the header or its absence): `initialize`, `ping`, `tools/list`,
`tools/call`, no session minted. The `Operations` port grew from §6's three methods to
six: `visible`, `describe`, `call` (with `CallMode` `preview`/`apply`/`propose`, default
`apply`, answering `OperationResult::Response {status, body}` or `Proposed {proposal,
url}`), `whoami`, `proposal`, `proposals`; the last three answer HTTP-shaped like `call`,
so `hypha-http` implements them over its own routes. Tool results keep Hypha's body as
is (the error body too, `isError: true`), the status in `_meta["hypha/status"]`, and
`structuredContent` only for objects. `tools/list` is never paged (6 + at most the
curated tools), so any cursor is invalid; its `ttlMs` is `ServerConfig::list_ttl_ms`
(60 s), `private`. A curated tool appears, and can be called, only while `visible`
reports its operation; otherwise it's `Unknown tool`.

**Direct tools' names: each `.` as `__`** (`admin.products.update` →
`admin__products__update`), always. MCP allows dots, but the clients that matter hand
tool names to model APIs, and the Anthropic and OpenAI APIs take only
`^[a-zA-Z0-9_-]{1,64}$`; some clients also prefix them (Claude Code's
`mcp__<server>__<tool>`). The mapping reads back because `OperationId` refuses `__` and
an `_` beside a `.` (otherwise `a._b` and `a_.b` would share `a___b`); no current
operation ID has either. A curated tool whose name passes 64 characters stops
`Server::new` (`direct_tool_name_too_long`), never truncated. The longest current
operation ID mapped is `admin__notifications__set_template_enabled`, 42 characters, 54
with Claude Code's `mcp__hypha__`. Only tool names are mapped: `find_operations`
answers, `describe_operation` and `call` take and show the dotted ID, and
`find_operations` gives a curated operation's tool name beside it (`tool`). The fixed
tools' names have no dots and are unchanged.

## As built: hosting agents (M2–M4)

- **Tokens are stored, hashed** (`preview_tokens`, migration 0070), not sealed. A token
  must stay short whatever the write created (a sealed token carrying 300 IDs is a 6 KB
  header, past what proxies take), it must be single use (which needs a write anyway), and
  storing it makes "used" a row update inside the apply's own transaction: two applies
  racing with one token serialize on its row, and a write that rolls back (stale, failed)
  leaves it unused. The record (`hypha_core::PreviewToken`, port `PreviewTokenRepo`) sits
  in core beside idempotency keys: a mechanism every command shares, owned by none. It's
  bound to the actor and to the request's fingerprint (method, path, query and canonical
  body, `extract::fingerprint`, the one idempotency keys use), which is stricter than "the
  operation": a token for product A can't apply to product B even if no event would tell.
  It's issued after the preview's ROLLBACK, on the same connection (`PgPreview::finish`),
  so a preview stays one connection. A preview with `complete: false`, or of a route
  without credentials, gets none.
- **One commit path for applies.** A write sent with `Hypha-Preview-Token` runs like a
  preview, in one outer transaction (`PgStore::begin_apply`: each write's transaction a
  savepoint), so the whole request commits or nothing does; once the handler is done its
  events are read back by `seq` (as M1 does), digested (`preview::digest`) and compared:
  equal, `COMMIT` and the dispatch hook told; different, `ROLLBACK` and `409
  preview_stale` with a fresh preview built from the run and a token for it (its `emails`
  and `calls` are empty: in an apply those effects ran). This is the `Tx`/`end_transaction`
  path M1 described, lifted to the request: checking per transaction would commit the first
  of a request's writes before the last could be compared. The token is claimed in
  `Caller::authenticate` (and `StaffCaller`), in that transaction; the preview's IDs are
  handed out first by `HostIds` (`preview::applying`), then fresh ones. A used token is
  `409 preview_used`; a retry of an applied write sends its `Idempotency-Key` without the
  token to get the replay (with the token it would be refused before the key is read).
  Known gap: an email sent while answering (an invite) goes out before a stale apply rolls
  back; holding emails until the commit is a follow-up (done: "As built: review fixes").
- **The operations index** (`hypha-http`'s `operations.rs`, `AppState::operations()`): the
  admin rows of the route table and the build's spores' (`Deployment::spore_rows`, now on
  the Workers deployment too), each with its `OperationId`, method, path, `write`,
  `x-hypha-scopes`, summary, description and tag; `get(id)`, `find(method, path)` (the
  fixed segment wins, as the router decides), `Operation::request_path(params)`
  (percent-encoded, a catch-all keeps its slashes). `/mcp`'s `visible`/`describe` read it.
- **Internal dispatch** (`internal.rs`): each host installs its router once built
  (`AppState::install(D::router(), D::spore_rows())`, router without its state, so the
  state holds no reference to itself); `internal::dispatch(state, request, agent)` sends a
  request through it with the state, outside any preview or apply of the request sending it
  (`preview::detached`). An agent rides as the `AgentRequest` extension (agent and `Ctx`),
  which only in-process requests can carry; `Caller` builds the agent's `Ctx` from it,
  `StaffCaller` refuses it (`session_required`), the storefront too.
- **Agent authentication and modes, in one place: `agents::call(state, agent,
  OperationCall)`**, not a layer. `/mcp` authenticates the credential to an `AgentId` and
  calls it; nothing else makes an agent's request. A layer would have had to rebuild the
  operation from the HTTP request, open ports twice and still not see proposals, which wrap
  the whole request; here the operation is known by ID, the request is built from it, and
  the proposal stored after it. An agent's key (`hypha_ak_…`, an `Identity` with
  `Principal::Agent` and `Credential::AgentKey`, migration 0070 allows the principal) sent
  to any route is `401 agent_credentials_mcp_only`; `hypha_auth::authenticate_agent_key`
  is what `/mcp` will call. The `Hypha-Propose` header of the brief isn't needed: agents
  never send HTTP themselves; `Mode::Propose` is the same ask.
- **Proposals.** Propose = the request sent with `Hypha-Preview: 1`; a `200` is kept with
  its preview (token removed, the IDs it was handed added as `ids`, hidden from the DTO),
  the digest (`PreviewDigest`, a response extension beside `PreviewIds`), the operation's
  scopes and the idempotency key, answering `202 {"proposal": …}`. Approving makes a token
  from the proposal's own IDs and digest (so nothing expires between a 7-day proposal and a
  15-minute token: "re-previewed if the token expired" isn't needed) and sends the write as
  the agent with it, past the agent's propose mode but not past the cap `off`. A proposal
  whose preview was `complete: false` is sent without a token (its full run records more
  than its partial preview, and an external effect must not run before a rollback).
  `applied`/`failed` are recorded by the approver; stale is recorded `failed` and answered
  `409 preview_stale` with `details.preview` and `details.proposal`. Approving can't be
  previewed (`blocked_by: {"kind": "proposal"}`): its write is its own request.
- **The proposal's ID beside the actor** needs a column (`outbox_events` has the actor
  only, and `data` is the contract payload): a follow-up. The write's events are the
  agent's (`Actor::Agent`); `proposal.decided` names the approver.
- **Settings.** `[agents]`: `enabled`, `writes`, `proposal_lifetime` (1 h to 90 d),
  `preview_token_lifetime` (1 min to 1 d), `preview_timeout`;
  `jobs.prune.agent_calls_after`; `[rate_limits]` `agent_call` (120, then 2 a second) and
  `agent_write` (20, then one per 3 s), with their Workers bindings. The prune job (both
  hosts) also expires proposals and deletes expired preview tokens.
- **Follow-ups.** `/mcp` and the agent client kind in OAuth (next); emails of a stale apply;
  the proposal ID in the outbox; bulk approval; e2e of a propose/approve round trip once
  `/mcp` gives agents an HTTP way in (M8).

## As built: /mcp and connecting clients (M5)

- **The endpoint.** `POST /mcp` is a hidden row of the route table (in no OpenAPI
  document, like `/oauth/*`), so both hosts serve it; GET and DELETE are the router's
  `405`. The transport (`routes/mcp.rs`) checks, in order: `agents.enabled`; `Origin`
  (absent is a non-browser client; present, it must be the API's own origin or
  `server.admin_url`'s, else `403 origin_not_allowed`); the bearer, an autonomous agent's
  `hypha_ak_…` or a connected agent's `hypha_at_…` (anything else, staff sessions and
  apps' tokens included, is `401 invalid_token`, and no credential `401 unauthenticated`,
  both with `WWW-Authenticate: Bearer resource_metadata="<issuer>/.well-known/oauth-
  protected-resource/mcp"`); the agent active now (`agents::acting`, also used by
  `agents::call`); `Content-Type: application/json` (`415`); `server.max_body`. Then
  `hypha_mcp::Server::handle` (one `Server` per process, `mcp::server()`).
- **`Operations` (`hypha-http`'s `mcp.rs`, `AgentOperations`).** `visible` is the
  operations index through `mode_for`: callable by agents at all
  (`Operation::for_agents`: not public, not `x-hypha-staff-session`), in the allowlist,
  every scope held (`Permissions::mode_for`), writes dropped under the cap `off`. The
  `x-hypha-staff-session` extension (already on `/me` and logging out) now marks every
  `StaffCaller` route (approving and rejecting proposals, app session tokens, private
  apps, the connection routes), so a person's own acts are never an agent's operation.
  `describe` reads `Operations::docs`: the admin document (with the build's spores' rows)
  built once, each operation's path and query parameters as objects, its JSON body and
  its success answer, each made self-contained (every `#/components/schemas/X` it
  reaches copied into its own `$defs` and pointed there; a test runs it over every
  admin operation and finds no reference outside). `call` hands an operation that exists
  and is callable by agents to `agents::call` even when `visible` hides it, so the
  refusal is the precise one (`agent_writes_off`, `operation_not_allowed`, the route's
  `missing_scope`) and is logged; a `202 {"proposal"}` becomes `Proposed` with
  `{admin_url}/agents/proposals/{id}`. `whoami` answers the agent (name, kind, the person
  it acts for, its client), its `Permissions` now, the cap, the store (name, time zone,
  language) and its markets with their currencies and minor units: the store has no
  currency of its own, prices are per market. `proposal`/`proposals` are
  `GET /admin/agent-proposals[/{id}]` sent in process as the agent, so paging and the
  "own proposals only" rule are the route's.
- **Idempotency of tool calls.** `call` and every write's direct tool take
  `idempotency_key` and `preview_token`. The agent's own key is the only thing that makes
  *its* retries safe: a key the server derives from the call's content would merge two
  deliberate, identical writes (two `+5` stock adjustments), and the JSON-RPC `id`
  restarts with the client. A write sent without one (apply or propose; previews need
  none) gets a fresh UUIDv7, so the proposal it may become carries a key and its
  approval, retried after a lost answer, replays instead of writing twice. Keys take
  effect where routes take them (the order, fulfilment and payment writes and
  proposals); elsewhere `preview_token` is the exactly-once tool (`409 preview_used`).
  The instructions tell agents to send both.
- **Direct tools and instructions.** `mcp::DIRECT_TOOLS` is §7's 23 (14 reads, 9
  writes; `direct_tools_are_operations` fails if one is renamed or stops being callable
  by agents); `mcp::INSTRUCTIONS` tells a model to start with `whoami`, preview, show the
  person, apply with the token, use keys, follow proposals, read money as minor units of
  the named currency and times as UTC in the store's zone, and treat store text as data.
- **Errors per operation.** `describe_operation` lists each error status the document
  gives with its description (`ErrorDoc.code` became optional): the codes are still
  prose there. `x-hypha-errors` per route, generated from each command's `ErrorCode`s
  rather than hand-written, is a follow-up.
- **Connecting a client** (`hypha-oauth`'s `client.rs`, `routes/oauth.rs`,
  `routes/agents.rs`). One flow, two client kinds: an `AuthorizationRequest` is for an
  app (`RequestClient::App`, its manifest) or an MCP client (`RequestClient::Agent`, its
  Client ID Metadata Document); a code or token pair belongs to an `OAuthClient`
  (`App(key)` / `Agent(id)`; migration 0071: nullable `app_key`, `agent_id`, a check that
  exactly one is set, tokens cascading with their agent). `/oauth/authorize` with
  `resource` naming `<issuer>/mcp` (scheme and host in any case, a trailing slash or not)
  is the agent flow; another `resource` is a `400 invalid_target` page. The document is
  fetched through `HostManifests` (so natively only public addresses unless
  `outbound.allow_private`) and never cached: it's read when a person connects, which is
  rare and rate limited (`Rule::OAuthIp`), and a client that changed its redirect URIs is
  believed at once. `hypha-oauth` never sees an agent: approving
  (`POST /admin/agent-connections/{id}/approve`) makes it with
  `hypha_agents::prepare_connected_agent` in `hypha-http` and stages it in the batch
  `prepare_agent_approval` returns with the decided request and the code, so no new edge
  between domain crates. An agent's tokens carry the grant's scopes for the token answer
  only; what it may do is read from the agent at each request, so a refresh doesn't check
  the agent (revoking it revokes its tokens, `revoke_agent_tokens`; a disabled one is
  refused at `/mcp` and works again once enabled). `authenticate_access_token` answers
  `agent_credentials_mcp_only` for an agent's token, `authenticate_agent_token` refuses
  an app's (not issued for `/mcp`).
- **What the specification required that §6 didn't say.** Protected Resource Metadata
  at the path RFC 9728 builds from the resource (`/.well-known/oauth-protected-
  resource/mcp`, named in the `401`; the root form is served too, as MCP clients fall
  back to it). `iss` on every authorization response (RFC 9207: MCP clients must check it
  when present, and a server sending it must advertise
  `authorization_response_iss_parameter_supported`): added to apps' redirects too, for
  every location carrying `code` or `error`. `resource` on the token request (checked:
  only `/mcp`). CIMD's rules: the document's `client_id` equal to its URL, the URL `https`
  with a path and no fragment, credentials or dot segments; `client_name` and
  `redirect_uris` required; no secret, `token_endpoint_auth_method` `none`; redirect URIs
  `https` or `http` to a loopback host, matched exactly except a loopback one's port
  (OAuth 2.1 / RFC 8252 §7.3: a native client listens where it can); the consent page
  shows the redirect's host and warns when every redirect is on the person's machine.
  `offline_access` and other non-Hypha scope names a client sends are dropped, never an
  error (clients add them by habit). Dynamic Client Registration is deprecated by the
  2026-07-28 revision and not offered: a client that only knows it uses an autonomous
  agent's key as a header.
- **Tests.** `route_tests/mcp.rs` speaks MCP over HTTP: both eras' `tools/list` differing
  by grant, find/describe/read/`whoami`, preview, propose → `get_proposal` → approved by
  the owner → applied, an `isError` result carrying Hypha's body, the token applying once,
  the cap `off`, the transport rules (401 with the challenge, staff and key tokens
  refused, a foreign `Origin`, 415, 405, a disabled agent, `agents.enabled = false`),
  and the whole OAuth connection against a metadata document the test serves: authorize
  (a loopback redirect at an unlisted port, a wrong resource, an unlisted host), the
  consent page's read, approve, the code (wrong verifier refused), tokens, `/mcp` as the
  agent and refused elsewhere, refresh rotation, reuse revoking, revocation by the
  agent's revoke and `/oauth/revoke`, deny with `access_denied` and `iss`.
  `hypha-oauth/tests/agents.rs` covers the commands; the oauth contract gained an
  agent-client scenario. `scripts/e2e.sh`'s `## mcp` creates an autonomous agent and,
  over `/mcp`, lists tools, `whoami`, reads, previews, proposes, then the owner approves
  and the agent reads its proposal applied.
- **Follow-ups.** `x-hypha-errors`; an `Mcp-Session`-free SSE answer for long calls
  (MCP Tasks, §12); app actions as `app.<key>.<action>` operations (M6); the Workers
  check of `/mcp` (M8); caching metadata documents if connecting ever becomes frequent.

### After M5: what the admin's pages asked for

- **A stale approval is a second look, not a failure.** Approving a proposal whose write
  would no longer record what was previewed writes nothing and answers `409
  preview_stale` as before, but the proposal stays `pending`: `Proposal::refresh` gives it
  the fresh run's preview (with the IDs that run was handed) and digest, counts
  `refreshes` (migration 0072) and records `proposal.updated` (`{proposal, agent,
  refreshes, by}`); `details.proposal` is the updated proposal. Approving again applies
  the new version exactly. A fresh run that fails (the write would error) still marks it
  `failed`. The stale apply's response carries its run's IDs and digest
  (`PreviewIds`/`PreviewDigest`, as a preview's does) for this. Its `emails` and `calls`
  are empty (in the apply they ran), as for any stale answer.
- **Summaries.** `ProposalResponse` and the call log's items carry `summary`, the
  operation's from the operations index (`null` once this build has no such operation).
- **Grant errors say where.** `agent_scope_not_grantable`, `not_allowed_to_grant`,
  `not_allowed_to_grant_apply`, `invalid_grant`, `unknown_scope`, `invalid_write_mode`
  and `invalid_operation_id` answer `details: {field, scope?, operation?}` (`field`:
  `read`, `write`, `customer_data`, `operations`; `GrantError::field`, `GrantField::of`).
- **`GET /admin/agent-settings`** (`agents:read`): `{enabled, writes,
  proposal_lifetime_seconds, preview_token_lifetime_seconds}`, the operator's `[agents]`,
  read-only through the API.

## As built: review fixes

- **A write an agent only proposes never commits without a person.** The mode of a write
  is decided by the write scopes the agent holds (`Permissions::mode_for`: any held only
  to propose makes it a proposal), never by a missing read scope, which used to make
  the call a direct write the route committed when its command didn't ask for that
  scope (`admin.carts.set_customer` with `{"customer": null}` and no `customers:read`,
  under the cap `propose` too). And by construction: a direct read or write runs with
  `Permissions::applied_scopes` (no write scope held only to propose); only a preview,
  a proposal's preview (both sent with `Hypha-Preview`, so rolled back) and an approved
  proposal's write carry them. `holds_all` is what `visible` asks.
- **Narrowing an allowlist is the only free change.** `update_agent` asks
  `require_grantable` for any grant change but a strictly narrower allowlist (one where
  there was none counts as narrower): widening or lifting it hands the grant out again.
- **Approving is the person's act.** `agents::apply_approved` spends none of the
  agent's rate limits, doesn't note its use and isn't in its call log (the proposal
  records who approved and what the write answered; the outbox stamps the write with
  the agent). Only a definitive refusal (a 4xx but `408`/`429`) marks the proposal
  `failed`; a `408`, `429` or 5xx is answered as is and the proposal stays pending.
- **A replayed approval is applied.** An apply whose answer replays its
  `Idempotency-Key` (`idempotent-replayed`) is answered as is, with nothing compared or
  kept: a retried approval, or two people approving at once, records the proposal
  `applied` with the stored answer instead of calling it stale. A route without keys
  can't tell its own earlier write from someone else's: a retried approval there is
  stale (the record's version moved) and shows what the write would do now.
- **What applying compares** (§3.3, §11: "comparing more later would refuse writes that
  pass today"; the live test showed why it must). The digest covers the events and the
  rows of aggregates the write wrote, each with the version it found (`Inserted` for a
  new one): an apply over anything changed since its preview is stale even when its
  events read the same (`product.updated` names the fields changed, not their old
  values, so an approval after the owner retitled the product overwrote the owner's
  title). Recorded in Postgres, not in each adapter's write path: the trigger
  `hypha_note_write` (migration 0073) on every aggregate's table (a test holds every
  table with a bigint `version` to it) logs table, key and the version found into the
  transaction-local setting `hypha.writes`, only while `hypha.log_writes` is on (previews
  and applies set it), so a write rolled back to its savepoint takes its line with it, as
  its rows. `PgPreview::recorded` reads both back (`hypha_core::WriteRecord`). Previews
  run only on Postgres, so the in-memory adapter has nothing to diverge on. Proposals
  previewed before this answer stale once when approved, and take the new digest.
  Spores' own versioned tables aren't covered unless their migrations add the trigger.
- **Apply does what was previewed or nothing, effects included.** An apply holds its
  emails and background work (`Apply::hold_email`, `defer`) and runs them after the
  COMMIT; a rollback drops them, and a stale apply lists the held emails in its fresh
  preview. A held email the provider refuses after the commit is logged: the committed
  write's answer can't change. Calls out: an apply's preview was complete, so it made
  no call but to read-only extension points and touched no media storage. The apply
  allows exactly that (read-only points are called again, as in a preview); any other
  call (`payments.capture`, a webhook, a manifest), media storage or an upload URL is
  blocked before it goes out, and the apply answers `409 preview_stale` with a fresh,
  incomplete preview naming it in `blocked_by` (no token: such a write goes through a
  proposal, whose approver sees what couldn't be previewed). Comparing a call against
  the preview's record instead would allow nothing more: a complete preview has none.
- **`/mcp` finds the agent once per request.** `serve` calls `agents::acting` (with
  `NoteUse::Yes`, which notes its use on the same connection) and hands the `Acting` to
  `agents::call_as` for each tool call. Connections a direct `tools/call` takes from the
  pool: 6 before (the credential, `acting` in `serve`, `acting` again in `judge`,
  `judge`'s for the use, the routed request, the log), 4 now (the credential, `acting`,
  the routed request, the log); a proposal takes one more for itself, as before.
- **Preview tokens keep the path's case.** `extract::fingerprint` no longer folds it;
  idempotency keys still do, in `KeyedJson`, as before the fingerprint was shared, so
  their stored records match their retries.
- **A connected agent's refresh token works for its client only.** Dropped in the second
  review: a public client's refresh token is a bearer credential whichever client ID is
  named, so the column and the check bought nothing (see below).
- **Direct tools: 23.** `admin.inventory.list_items` (stock across products, `below`).

## As built: second review fixes

- **A preview reads no more than the routes, and fingerprints after authenticating.**
  The middleware reads a previewed or applied write's body before the caller is known,
  so it's capped at `server.max_body` (`413 body_too_large`, as the routes answer;
  `Deployment::router(max_body)` hands it the limit), and the request is fingerprinted
  (canonical JSON) only when a token is claimed or issued.
- **A direct tool's `$ref`s resolve.** Each part (params, query, body) stood alone with its
  own `$defs` nested under `properties`, where `#/$defs/X` resolves against the input
  schema's root: the parts' `$defs` are hoisted into one root `$defs` (12 of 22 tools
  were broken).
- **Times counted in microseconds are times.** The digest blanks numbers under a key
  ending in `_at` too (`customer.erasure_requested`'s `due_at`), so an agent's erasure
  proposal (`customers:erase` is propose-only) can be approved. And it takes the rows
  written sorted, since a multi-row `UPDATE … FROM unnest` writes them in the plan's
  order.
- **A proposal's write is shown only to who may see all of it.** `request`, `preview` and
  `response` need, for a caller other than a staff session, every scope the write takes
  and every `customer_data:*` scope; otherwise they're absent and listed in `redacted`.
  The proposing agent is no exception: what it previewed may hold more than its own
  redaction would show it.
- **Approving claims the proposal.** `approve_proposal` moves it to `applying`
  (version-checked; approver and time in `decided_by`/`decided_at`), which a second
  approval, a rejection and the expiry job refuse (`409 proposal_applying`) until the
  write's answer is recorded (`applied`, `failed`), it's previewed again (`refresh`) or
  let go (`release`: the write may go through later, or the operator turned agents off
  or capped them to reading meanwhile, which no longer fails the proposal). An approval
  that died leaves it applying for `APPLYING_LEASE` (5 minutes), after which it counts as
  pending. The approval names the preview the person saw: `{"refreshes": n}`, another
  count is `409 proposal_changed` with the proposal as it is (the admin sends it and
  shows the new preview). Migration 0074.
- **Leaner.** `Preview` and `Apply` are one `Run` with an optional apply part; one
  thread-local; one ending. The write log's trigger fires only while logging (its
  `WHEN`). A preview's pooled connection dropped half-way is closed rather than pooled,
  so its locks go at once. `/mcp` opens one connection for the credential and `acting`
  and one after the routed request for a proposal and the log. The sales report sums
  refunds in SQL. Gone: `Operations::find`, `agents::call` (tests use `acting` and
  `call_as`), `StageProposals`, `StageAgents::update_agent`, `create_*_agent`, the hand-made
  base64, `DEFAULT_INSTRUCTIONS`, `CallArgs`, the tokens' client ID (migration 0073 is
  now the write log).
- **Listing inventory items takes `products:read` too**: it names products and SKUs.
