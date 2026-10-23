# 0036: Releases and distribution

- **Status:** Accepted (2026-10-23). Danilo's answers are in §9; build order §10.
- **Scope:** the project's name and namespaces, what is published where, how versions
  and tags work, how a release is made, what a version promises, and how people start
  from Hypha: run it, build around it, or change it.
- **Not in scope:** the license (Danilo is still deciding; it must be settled before the
  first publish, §8), crates.io (§4.3), hosted Hypha.
- **Related:** 0010 (OpenAPI, the SDK), 0011 (the TypeScript apps), 0025 (hosting, the
  CLI), 0026 (Workers), 0029 (apps, their packages, co-located apps), 0030 (spores).

## 1. Why

Hypha is meant to be pulled down and used three ways: run as it is, built around (apps
and spores), or changed (starting from the admin or storefront, or forking the whole
repository). Today none of that works for someone outside this repository: the npm
packages are private and export TypeScript source, the CLI assumes a checkout and
compiles the API on the server, and a spore can only be built inside the workspace.

## 2. Name and namespaces

The product is **Hypha**; its formal name, and the one identifier everywhere else, is
**Hypha Commerce** / `hyphacommerce`:

| What | Name |
| --- | --- |
| Website | `hyphacommerce.com` (`hypha-commerce.com` to redirect, once there are sponsors) |
| GitHub | `github.com/hyphacommerce` (`hypha`, `app-stripe`, `app-meilisearch`, `spore-stripe`, templates) |
| npm | `@hyphacommerce/*` |
| Images | `ghcr.io/hyphacommerce/*` |
| Rust crates | `hypha-*` (free on crates.io; not published yet, §4.3) |

`@hypha` on npm and the `hypha` crate belong to others, and "Hypha" alone is registered as
a trade mark by several owners in other trades (an EU/UK mark in classes 9, 37, 38; a UK
one in 35, 41, 42). The compound name had no conflicts in the EU, UK or WIPO searches.
Code and documentation say "Hypha"; READMEs, the website and package metadata say
"Hypha Commerce".

## 3. Three ways in

| Who | Starts with | What we ship |
| --- | --- | --- |
| Runs it as it is | `npx @hyphacommerce/cli init`, then `deploy` | images, the CLI |
| Builds around it | an app (any language, over HTTP) or a spore (Rust, compiled in) | the SDK packages, an app template, a spore template, the guides |
| Changes it | the storefront or admin as a starter, or a fork | `cli create`, a repository that's easy to fork |

The **storefront** is meant to be copied: every shop customises it. The **admin** is
meant to be run as its image and extended through apps (blocks, embedded pages, actions);
copying it works, but every upgrade becomes a merge, so the guides present it as the
escape hatch.

## 4. What is published where

### 4.1 Images (GHCR, `linux/amd64` and `linux/arm64`)

- `hypha-api`: the stock host (`host! { spores: [] }`).
- `hypha-admin`, `hypha-storefront`: the apps built as Node servers (`HYPHA_TARGET=node`).

Tags: `0.4.0`, `0.4`, `latest` for a release; `next` for a pre-release. The CLI pins the
images to its own version, so `deploy` pulls rather than compiles (today's build from
source stays as `--build-from-source`, and is what a deployment listing spores from
outside gets, since its host crate is its own). A build with this repository's spores is
no longer needed once the Stripe spore lives in its own repository (§6).

The Workers build (the API's `.wasm` bundle) is attached to the GitHub Release;
`deploy --target cloudflare` uses it, or builds it when the deployment has spores.

### 4.2 npm

Published, with provenance:

| Package | For |
| --- | --- |
| `@hyphacommerce/cli` | deploying, and `create` (§7) |
| `@hyphacommerce/sdk` | custom storefronts and admins: the API's types and clients |
| `@hyphacommerce/app` | app authors |
| `@hyphacommerce/app-bridge`, `@hyphacommerce/app-bridge-host` | embedded app pages; the host side for custom admins |
| `@hyphacommerce/app-blocks` | rendering app blocks |
| `@hyphacommerce/storefront-elements` | mounting app elements in a storefront |

Not published: `admin`, `storefront` (applications: images and starters), `emails`
(builds the templates the API embeds).

Each published package gets a build (tsdown: ESM JavaScript and `.d.ts` in `dist/`):
Node refuses to strip types from files under `node_modules`, so TypeScript source can't be
shipped as it is. Inside the workspace nothing changes: the `exports` map keeps a
`source` condition pointing at `src/`, which the apps and tests resolve, and `publishConfig`
points the published package at `dist/`.

### 4.3 Rust crates: not yet

The only outside Rust users are spore authors, who need `hypha-spore`. crates.io needs
every dependency published too, which here means about forty crates (core, every domain,
`hypha-http`, `hypha-store-postgres`), each then a semver promise while the spore API still
changes every week. Until it settles, spores depend on a git tag:

```toml
hypha-spore = { git = "https://github.com/hyphacommerce/hypha", tag = "v0.4.0", features = ["http"] }
```

The CLI's host crate for spores from outside (`.hypha-build/`) already works this way.
When the spore API is stable, the whole set is published with release-plz, in lockstep.

## 5. Versions, tags and releases

- **One version for everything that ships together**, one tag per release: `v0.4.0`.
  The API, images, CLI and SDK come from the same OpenAPI documents and are only right
  as a set. Contracts that need their own versions have them already: each extension
  point (`VERSION`), each event type, the manifest schema, the block format (v1). A
  library that becomes protocol-stable while the server moves on (`app-bridge`,
  `app-blocks`) can split off later with tags of its own (`app-blocks-v1.2.0`).
- **SemVer in 0.x**: the minor number for breaking changes (`0.4` → `0.5`), the patch for
  fixes and additions. A pull request labelled `breaking-api` (the API compatibility
  check) means the next release is a minor. `1.0` waits until the HTTP API and the spore
  API are both ones we'll keep.
- **Pre-releases**: `v0.5.0-rc.1`: npm dist-tag `next`, image tag `next`.
- **Branches**: releases come from `main`. A `release/0.4` branch is made only to
  backport a fix to an older minor.
- **How**: release-please (we already write Conventional Commits). Merging to `main`
  keeps a "Release v0.5.0" pull request open with the changelog and every version bump
  (the root `Cargo.toml`'s `workspace.package.version`, each `package.json`, the CLI's
  image pin), through its Cargo and Node workspace plugins, linked. Merging it tags
  `v0.5.0` and makes the GitHub Release; the tag runs `release.yml`: images to GHCR,
  npm packages with provenance through trusted publishing (GitHub OIDC, no npm token),
  the Workers bundle attached.

## 6. Repositories

| Repository | Holds |
| --- | --- |
| `hyphacommerce/hypha` | the API (every crate), the npm packages, the CLI, the admin, the storefront |
| `hyphacommerce/spore-stripe` | the Stripe spore: the example of a spore built outside, depending on a git tag |
| `hyphacommerce/app-stripe`, `hyphacommerce/app-meilisearch` | example apps (`app-stripe` beside `spore-stripe`: the same feature both ways) |
| `hyphacommerce/spore-template`, `hyphacommerce/app-template` | minimal starting points (`cargo generate`, `cli create app`) |

The admin and storefront stay in the main repository: CI tests them against the API at
the same commit, and they change with it. The Stripe spore moves out now, before the first
tag, since showing a spore built outside was the point of it; until there's a tag it
depends on `hypha`'s `main`, so a change to the spore API needs matching pull requests in
both repositories. Its route tests go with it, on a public test harness
(`hypha_host_axum::testing`, feature `testing`) that any spore author can use. Core CI
keeps covering spores through `test_spore.rs`; the hosts' `spore-stripe` feature gives
way to the CLI's host crate, and `HYPHA_SPORES=stripe pnpm dev` builds one from a
`spore-stripe` checkout beside this one.

## 7. Starters: `cli create`

`npx @hyphacommerce/cli create storefront my-shop` (and `admin`, `app`) downloads the
subdirectory at the release's tag and swaps each `workspace:*` dependency for that
release's published version. No mirror repositories to keep in step. For that to keep
working, the apps must build outside the workspace (today they lean on its TypeScript
sources and the root lint config), and CI checks it: each starter is created in a
temporary directory, installed against the packages packed from the same commit, and
built.

## 8. What a version promises

Written up for users in `docs/releases.md`:

- **Covered by semver:** the HTTP API (held by the API compatibility check), event
  payloads, extension points, the manifest schema, `hypha.config.toml`'s keys, the npm
  packages' exports.
- **Not yet:** the Rust spore API (`hypha-spore`), until crates are published.
- **Upgrades:** migrations are append-only, so any release upgrades straight to any later
  one. CI can prove it later by migrating a database made by the previous tag.
- **The license** applies to every published version for good, so it is settled before
  the first publish.

## 9. Decisions

1. **Name:** Hypha, formally Hypha Commerce; `hyphacommerce` everywhere (§2). Domain
   bought 2026-10-23.
2. **Versions:** one version, one tag per release (§5).
3. **crates.io:** not until the spore API is stable (§4.3).
4. **Stripe spore:** its own repository, now (§6).
5. **Admin and storefront:** published as images and as starters through `cli create`;
   the SDKs published (§3, §4, §7).
6. **License:** open; before the first publish.

## 10. Build order

1. R1: the npm scope `@hyphacommerce` (done) and the repositories moved to the
   `hyphacommerce` organisation, the CLI's first-party app URLs with them.
2. R2: the package builds (tsdown, `source` condition, `publishConfig`), checked in CI
   by packing each package and importing it from a temporary project.
3. R3: release-please (config, manifest, linked versions) and `docs/releases.md`.
4. R4: `release.yml`: the images (multi-arch), npm publish, the Workers bundle.
5. R5: the CLI on prebuilt images (`--build-from-source` kept).
6. R6: `cli create` for the storefront, admin and app, with the starter check in CI.
7. R7: the templates (spore, app).

The Stripe spore's move (§6) comes right after R1: the test harness, the repository split
with its history, then removing it here (hosts, CI, scripts, the CLI's first-party
spores, the docs).
