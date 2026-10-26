---
title: Deploy it
description: Deploy Hypha to your own server, this machine or Cloudflare Workers with its CLI.
---

Hypha's CLI writes a deployment folder with everything a deployment needs, then starts it.
It asks for what it needs and has a flag for every question, so it runs unattended too.

```bash
npx @hyphacommerce/cli@next init      # the deployment folder: target, domains, secrets
npx @hyphacommerce/cli@next deploy    # start it, or apply a change
```

`init` writes `hypha-deploy/`: `hypha.json` (the deployment's settings), `.env` (its
secrets, so keep the folder private), `hypha.config.toml` (the API's settings, typed by
the schema next to it) and, for a server, the Docker Compose and Caddy files. The folder
is the source of truth: `deploy` copies it to the server each time.

During the beta the CLI is on npm's `next` tag, which is why the commands say `@next`. The
CLI deploys the images released with it, so running a newer CLI is how you upgrade: the
next `deploy` pulls that version's images and the API migrates its database on start.

## Targets

| Target | What you get |
| --- | --- |
| `--target vps` | A server you reach over SSH: Postgres, file storage (SeaweedFS), the API, the admin, the storefront and Caddy for HTTPS, in Docker. The server pulls the published images (`ghcr.io/hyphacommerce/hypha-*`) for its own architecture, amd64 or arm64. `deploy` installs Docker if the server lacks it. |
| `--target local` | The same setup on this machine, under `*.hypha.localhost`. |
| `--target cloudflare` | The API, admin and storefront as Workers, Postgres through Hyperdrive (any Postgres you give it by URL), files in R2, set up with one Cloudflare API token. |

Point the domains at the server before deploying: Caddy gets the certificates from Let's
Encrypt as it starts, and `deploy` warns when a domain leads elsewhere.

## Cloudflare

`init` links to Cloudflare's token form with the permissions filled in, then checks the
token and finds your account and zone. `deploy` then:

1. prepares the database (migrations, the store, the owner, the default location);
2. creates what's missing: Hyperdrive, the KV namespaces, the R2 bucket and its domain;
3. deploys the API Worker, then the admin and storefront Workers bound to it;
4. connects the storefront to the API (its publishable key and webhook).

The Workers are built on the machine that deploys, so this target runs from a checkout of
the repository (below), with Rust, the WebAssembly target and `worker-build` installed.

## From a checkout

Some things build code rather than pull it, and need a clone of
[the repository](https://github.com/hyphacommerce/hypha) with its toolchain. Run the CLI
from it as `pnpm hypha …`:

- **The images from source**: `hypha init --from-source` (or `deploy
  --build-from-source`), built on the server, or here with `--build-on here` and copied
  over when the server is too small to compile Rust.
- **Spores**: a deployment with spores builds its own API, since the spores are compiled
  into it (`pnpm hypha spores add stripe`).
- **Co-located apps**: `pnpm hypha apps add stripe` (or `meilisearch`, or any app's git
  URL) builds the app from its repository and runs it beside Hypha.
- **The Cloudflare target.**

The installed CLI says so when it's asked for one of these.

## After deploying

- `status` shows what's running, and `logs [service]` follows the logs.
- The owner signs in to the admin with the email and password from `init`. Change the
  password first.
- Emails go to the log until you configure a sender: set `email.sender` to `smtp` or
  `resend` in `hypha.config.toml`, with its secret in `.env`, and deploy again.
- To upgrade, run the newer CLI's `deploy`. To change a setting, edit the file in
  `hypha-deploy/` and deploy again: Compose restarts only what changed.
