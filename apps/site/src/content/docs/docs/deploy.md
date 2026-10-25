---
title: Deploy it
description: Deploy Hypha to your own server or to Cloudflare Workers with its CLI.
---

Hypha's CLI writes a deployment folder with everything a deployment needs, then builds and
starts it. It asks for what it needs and has a flag for every question, so it runs
unattended too.

:::note
Until the first release, the CLI runs from a checkout of the repository: `pnpm hypha …`.
:::

```bash
pnpm hypha init      # the deployment folder: target, domains, secrets
pnpm hypha deploy    # build and start it
```

`init` writes `hypha-deploy/`, which git ignores: `hypha.json` (the deployment's
settings), `.env` (its secrets), `hypha.config.toml` (the API's settings) and, for a
server, the Docker Compose and Caddy files.

## Targets

| Target | What you get |
| --- | --- |
| `--target vps` | A server you reach over SSH: Postgres, file storage (SeaweedFS), the API, the admin, the storefront and Caddy for HTTPS, in Docker. The server pulls the images published with the CLI's version (`ghcr.io/hyphacommerce/hypha-*`); `--from-source` builds them from a checkout instead, on the server or here with `--build-on here`. A deployment with spores always builds its API. |
| `--target cloudflare` | The API, admin and storefront as Workers, Postgres through Hyperdrive (any Postgres you give it by URL), files in R2, set up with one Cloudflare API token. |
| `--target local` | The server setup on this machine, under `*.hypha.localhost`. |

## Cloudflare

`init` links to Cloudflare's token form with the permissions filled in, then checks the
token and finds your account and zone. `deploy` then:

1. prepares the database (migrations, the store, the owner, the default location);
2. creates what's missing: Hyperdrive, the KV namespaces, the R2 bucket and its domain;
3. deploys the API Worker, then the admin and storefront Workers bound to it;
4. connects the storefront to the API (its publishable key and webhook).

Compiling the API needs the `worker-build` tool and the WebAssembly target on the machine
that deploys.

## After deploying

- `pnpm hypha status` shows what's running, and `pnpm hypha logs [service]` follows the
  logs.
- Add apps that deploy beside Hypha with `pnpm hypha apps add stripe` (or `meilisearch`,
  or any app's git URL), and spores with `pnpm hypha spores add stripe`. The next deploy
  builds them in.

Emails go to the log until you configure a sender: set `email.sender` to `smtp` or
`resend` in `hypha.config.toml`, with its secret in `.env`.
