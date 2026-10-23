---
title: Connect an AI agent
description: Operate a store with Claude, ChatGPT, Cursor or your own agent, over MCP, with exact previews and approvals.
sidebar:
  order: 4
---

Hypha's admin API is also an [MCP](https://modelcontextprotocol.io) server, at `/mcp` on
the API. An agent connected to it can do a store's daily work (read orders, fix product
details, adjust stock, change prices) with two safeguards: **every write can be previewed
exactly**, and **a write it isn't trusted with becomes a proposal a person approves**.

Hypha runs no model. You bring the agent and choose its provider.

## What an agent is

An agent is a principal of its own (`agent_…`), with its own grant, call log and switch.
Everything it writes is recorded as its own. There are two kinds:

- **Connected**: a staff member connects an MCP client (Claude, ChatGPT, Cursor) through
  OAuth. The agent acts for that person: what it may do is its grant **within the
  person's role**, read fresh at every request. Narrow the role and the agent narrows;
  deactivate the person and it stops.
- **Autonomous**: created by staff for a job (a nightly restock script, an n8n workflow),
  with its own grant and a key, `hypha_ak_…`, shown once.

Agent credentials work at `/mcp` and nowhere else.

## Preview, propose, apply

Every admin write can run three ways:

- **Preview**: the write runs for real, in a transaction that's rolled back, with emails
  and calls to apps held back. The answer shows exactly what it would return and the events
  it would record. Nothing changes.
- **Apply**: the write happens. Sent with the preview's token, it commits only if it does
  exactly what the preview showed; if the store changed meanwhile, nothing is written and
  the agent gets the new preview.
- **Propose**: the preview is kept as a **proposal** in the admin's inbox. A staff member
  holding the write's scopes approves it, and it's applied exactly as previewed, or
  rejects it with a note the agent can read.

## Grants

```json
{
  "read": ["orders:read", "products:read", "inventory:read"],
  "write": { "products:write": "propose", "inventory:write": "apply" },
  "customer_data": [],
  "operations": null
}
```

- **read** scopes; **write** scopes, each `propose` (the default) or `apply`;
  **customer_data** scopes for names, emails, phones and addresses (none unless granted);
  **operations**, optionally the only operations the agent may call.
- Nobody grants more than they hold, and only a staff member's session grants `apply`.
- Some scopes are never granted to an agent: those that would let it widen its own reach
  or mint credentials (`users:write`, `api_keys:write`, `apps:write`, `agents:write`), and
  `checkout`. `customers:erase` can only be proposed.

The operator caps every agent in `hypha.config.toml`, whatever the grants say:

```toml
[agents]
enabled = true     # false: /mcp and every agent credential are refused
writes = "apply"   # "propose": no agent applies directly; "off": agents only read
```

## The tools

| Tool | Does |
| --- | --- |
| `whoami` | Who the agent is, for whom, what it may read and write, the store's markets and currencies |
| `find_operations` | Words to the operations it may call |
| `describe_operation` | An operation's inputs as JSON Schema, its answer, errors and scopes |
| `call` | Runs any operation, in `preview`, `apply` or `propose` mode |
| `get_proposal`, `list_proposals` | Its own proposals and their outcomes |

Beside them, direct tools for the daily work, such as `admin__orders__list` and
`admin__inventory__adjust_level`. Operations are the admin API's own, so everything in the
[admin API reference](/docs/api/admin/) is what an agent can do, with the same validation
and errors.

## Connecting a client

Clients that support OAuth with Client ID Metadata Documents connect with the server's URL
alone. The first time, the admin's connect page opens: you see the client's name and a
grant preset to read what you can and propose every write; change it, name the agent and
approve.

- **Claude Code**: `claude mcp add --transport http hypha https://api.your-shop.example/mcp`,
  then `/mcp` to sign in.
- **Claude**: Settings, Connectors, Add custom connector, with the URL.
- **ChatGPT**: with developer mode on, Settings, Apps & Connectors, Create, with the URL and
  OAuth.
- **Cursor**: in `.cursor/mcp.json`,
  `{"mcpServers": {"hypha": {"url": "https://api.your-shop.example/mcp"}}}`.

An autonomous agent sends its key as a bearer token instead:

```bash
claude mcp add --transport http hypha https://api.your-shop.example/mcp \
  --header "Authorization: Bearer hypha_ak_…"
```

## Safety

- **Plan for prompt injection.** An agent reads text strangers wrote: reviews, notes,
  addresses. Hypha assumes it will sometimes be fooled and limits the damage: writes are
  proposals unless you choose otherwise per scope, scopes that widen an agent's reach are
  never granted, the operator can make every agent propose-only or read-only, and each
  agent is rate limited.
- **Customer data is off by default.** Without `customer_data:*`, personal fields are
  absent from everything an agent reads.
- **Everything is logged.** Every tool call is listed per agent (operation, mode, status,
  duration), and the events it caused carry the agent as their actor.
