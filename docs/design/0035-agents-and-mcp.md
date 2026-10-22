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
take the same `mode`. Twenty-two, 13 reads and 9 writes:

| Job | Reads | Writes |
| --- | --- | --- |
| Orientation | `admin.setup.checklist` | |
| Orders | `admin.orders.list`, `admin.orders.fetch`, `admin.fulfillments.list` | `admin.fulfillments.create`, `admin.orders.mark_paid`, `admin.orders.cancel` |
| Customers | `admin.customers.list`, `admin.customers.fetch` | |
| Catalog | `admin.products.list`, `admin.products.fetch`, `admin.categories.list` | `admin.products.update`, `admin.products.update_variant`, `admin.products.set_status`, `admin.categories.add_products` |
| Stock | `admin.inventory.fetch_product_inventory`, `admin.inventory.list_locations` | `admin.inventory.adjust_level` |
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
