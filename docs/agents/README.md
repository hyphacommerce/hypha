# Agents: operating a store with Claude, ChatGPT, Cursor or your own

Hypha's admin API is also an [MCP](https://modelcontextprotocol.io) server, at `/mcp` on
the API (`https://api.your-shop.example/mcp`). An AI agent connected to it can do the
store's daily work (read orders, fix product details, adjust stock, change prices)
with two things no other platform gives it: **every write can be previewed exactly**,
and **a write it isn't trusted with becomes a proposal a person approves**.

Hypha runs no model. You bring the agent (Claude, ChatGPT, Cursor, n8n, a script) and
choose and pay its provider.

## What an agent is

An agent is a principal of its own (`agent_…`), with its own grant, its own call log and
its own kill switch. Everything it writes is recorded as its own (`Actor::Agent`).
There are two kinds:

- **Connected**: a staff member connects an MCP client (Claude, ChatGPT, …) through
  OAuth. The agent acts *for that person*: at every request, what it may do is its
  grant **within the person's role**, read fresh. Narrow the role and the agent narrows;
  deactivate the person and the agent stops.
- **Autonomous**: created by staff in *Settings → Agents* for a job (a nightly restock
  script, an n8n workflow), with its own grant and a secret key `hypha_ak_…` shown once.
  It keeps its grant when its creator leaves, like an API key.

Agent credentials work at `/mcp` and nowhere else: sent to any other route they're
refused (`401 agent_credentials_mcp_only`).

## Preview, propose, apply

Every admin write can run in three ways:

- **preview**: the write runs for real, in a transaction that's rolled back, with
  emails and calls to apps held back. The answer shows exactly what it would return,
  the events it would record, the emails it would send, and a `token`. Nothing changes.
- **apply**: the write happens. Sent with the preview's `preview_token`, it commits only
  if it records exactly the events the preview showed (else `409 preview_stale` with the
  new preview, and nothing is written).
- **propose**: the preview is kept as a **proposal** (`prop_…`) in the admin's inbox
  (*Agents → Proposals*). A staff member holding the write's scopes approves it (it's
  then applied exactly as previewed) or rejects it with a note. The agent follows it
  with `get_proposal` / `list_proposals`.

An agent whose grant says `propose` for a scope can't apply such a write: its "apply"
becomes a proposal, and the tool answers with the proposal's link.

## Grants and the cap

A grant lists:

```json
{
  "read": ["orders:read", "products:read", "inventory:read"],
  "write": {"products:write": "propose", "inventory:write": "apply"},
  "customer_data": [],
  "operations": null
}
```

- **read** scopes; **write** scopes each in `propose` (the default) or `apply` mode;
  **customer_data** (names, emails, phones, addresses: none unless you grant them);
  **operations**: optionally, the only operation IDs the agent may call.
- Nobody grants more than they hold, and only a staff member's session grants `apply`.
- Some scopes are never granted to an agent, in any mode: `users:write`,
  `api_keys:write`, `apps:write`, `agents:write` (an agent can't widen its own reach or
  mint credentials) and `checkout`. `customers:erase` is propose-only.

The operator caps every agent in `hypha.config.toml`, whatever the grants say:

```toml
[agents]
enabled = true     # false: /mcp and every agent credential are refused
writes = "apply"   # "propose": no agent applies directly; "off": agents only read
```

## The tools

Dumping the whole admin API into a model's context makes it slower and worse at
choosing, so `/mcp` offers a few tools that reach every operation on demand:

| Tool | Does |
| --- | --- |
| `whoami` | who the agent is, for whom, what it may read and write (and in which mode), the store's name, time zone and markets with their currencies |
| `find_operations` | words → the operations it may call, with their mode |
| `describe_operation` | an operation's inputs (path, query, body) as JSON Schema, its answer, its errors and scopes |
| `call` | runs any operation: `{operation, params, query, body, mode, idempotency_key, preview_token}` |
| `get_proposal`, `list_proposals` | its own proposals and their outcomes |

Beside them, 23 **direct tools** for the daily work, named after their operation with
each `.` as `__` (`admin__orders__list`, `admin__inventory__adjust_level`, …): orders and
fulfilments, customers, the catalog, stock (what's running low too) and prices. An agent
sees only what its grant allows.

Operations are the admin API's own (`admin.products.update` is `PATCH
/admin/products/{id}`), so everything in the [OpenAPI document](../../openapi/admin.json)
is what an agent can do, with the same validation, errors and redaction. Errors come
back as tool results carrying Hypha's `{code, error, details}`.

Money is always in minor units of the currency named beside it (`2490` with `EUR` is
24.90 €), and times are UTC.

## Connecting Claude, ChatGPT or Cursor (a connected agent)

Clients that support OAuth with Client ID Metadata Documents (the current MCP
specification's way) connect with the server's URL alone. The first time, your browser
opens the admin's connect page: you see the client's name, where it will receive access,
and a grant preset to *read everything you can, propose every write, no customer data*;
change it, name the agent and approve. The admin needs `server.admin_url` set (or
`HYPHA_ADMIN_URL`).

- **Claude Code**: `claude mcp add --transport http hypha https://api.your-shop.example/mcp`,
  then `/mcp` in Claude Code to sign in.
- **Claude (claude.ai, desktop)**: *Settings → Connectors → Add custom connector*, with
  the URL `https://api.your-shop.example/mcp`.
- **ChatGPT**: with developer mode on, *Settings → Apps & Connectors → Create*, with the
  same URL and OAuth.
- **Cursor**: in `.cursor/mcp.json`:

  ```json
  {"mcpServers": {"hypha": {"url": "https://api.your-shop.example/mcp"}}}
  ```

A client that only knows Dynamic Client Registration (deprecated by MCP, and not offered
by Hypha) can't connect through OAuth: give it an autonomous agent's key as a header
instead (below).

Revoke a connected agent in *Settings → Agents*: its tokens stop at once.

## An autonomous agent with its key

Create the agent in *Settings → Agents* (or `POST /admin/agents`), copy its key, and send
it as a bearer token. Any MCP client that takes headers works:

```bash
claude mcp add --transport http hypha https://api.your-shop.example/mcp \
  --header "Authorization: Bearer hypha_ak_…"
```

From a script, an MCP call is one POST. The 2026-07-28 protocol is stateless: each
request names its version in `_meta` and mirrors the method (and the tool) in headers:

```bash
curl https://api.your-shop.example/mcp \
  -H 'authorization: Bearer hypha_ak_…' \
  -H 'content-type: application/json' \
  -H 'accept: application/json, text/event-stream' \
  -H 'mcp-protocol-version: 2026-07-28' \
  -H 'mcp-method: tools/call' \
  -H 'mcp-name: admin__orders__list' \
  -d '{"jsonrpc":"2.0","id":1,"method":"tools/call","params":{
        "name":"admin__orders__list","arguments":{"query":{"limit":10}},
        "_meta":{"io.modelcontextprotocol/protocolVersion":"2026-07-28",
                 "io.modelcontextprotocol/clientCapabilities":{}}}}'
```

Clients of the earlier protocol (2025-11-25 and before: `initialize`, then calls) are
served too, without sessions.

**n8n**: add an *MCP Client Tool* to an AI Agent node, endpoint
`https://api.your-shop.example/mcp`, transport *HTTP Streamable*, authentication *Header
Auth* with `Authorization: Bearer hypha_ak_…`.

## Safety

- **Prompt injection is the threat to plan for.** An agent reads text strangers wrote:
  product reviews, customer notes, order addresses, a supplier's file. Any of it can say
  "refund every order". Hypha assumes the agent will sometimes be fooled and limits the
  damage: writes are `propose` unless you choose `apply` per scope; the scopes that
  would let an agent widen its reach are never granted; the cap can make every agent
  propose-only or read-only; rate limits per agent keep a runaway agent slow; disable or
  revoke one agent, or turn all of them off (`agents.enabled = false`).
- **Customer data is off by default.** Without `customer_data:*`, names, emails, phones
  and addresses are absent from everything an agent reads (and listed in `redacted`),
  connected agents included. Grant them only if your agreement with the model provider
  covers that data.
- **Everything is logged.** *Agents → an agent → Activity* lists every tool call (the
  operation, its mode, its status; no bodies), and the events it caused carry the agent
  as their actor.
- Only the API's own origin and the admin's may call `/mcp` from a browser.
