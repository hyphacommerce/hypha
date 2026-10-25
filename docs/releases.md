# Releases

Hypha ships as one set: the API, the admin, the storefront, the CLI and the npm packages
come from the same commit and are only right together, so there is **one version and one
tag per release** (`v0.1.0-beta.1`, later `v0.1.0`, `v0.2.0`, …). The API reports it in its
OpenAPI documents (`info.version`) and to MCP clients.

## Beta

Hypha is in **public beta** (`0.1.0-beta.N`). It works end to end and the demo at
[demo.hyphacommerce.com](https://demo.hyphacommerce.com) runs it, but it hasn't carried a
real shop's orders yet. During the beta:

- any release may change the HTTP API, event payloads, extension points or the
  configuration keys; each change is listed in the changelog;
- migrations are still append-only, so a database made by one beta upgrades to the next;
- there are no backports: fixes land in the next release.

The beta ends with `v0.1.0` once a few shops have run on it. From then on, 0.x follows the
rules below.

## Versions in 0.x

SemVer, with the 0.x reading: the **minor** number changes for incompatible changes
(`0.1` → `0.2`), the **patch** number for fixes and additions. A pull request labelled
`breaking-api` (the API compatibility check, `.github/workflows/api-compat.yml`) means the
next release is a minor. `1.0` waits until the HTTP API and the spore API are both ones
we'll keep.

Pre-releases are `v0.2.0-rc.1`: npm dist-tag `next`, image tag `next`, a GitHub
pre-release.

## What a version promises

Covered by the rules above:

- the HTTP API (`openapi/*.json`), held by the API compatibility check on every pull request;
- event payloads and extension points (each versioned on its own as well; a reshaped body
  is a new version, never a changed one);
- the app manifest schema and the block format;
- `hypha.config.toml`'s keys (a key is never renamed);
- the npm packages' exports.

Not yet: the Rust spore API (`hypha-spore`). Spores depend on a git tag
(`hypha-spore = { git = "https://github.com/hyphacommerce/hypha", tag = "v0.1.0-beta.1" }`)
and may need changes between releases until the crates are published.

Upgrades: migrations are append-only (an applied migration is never edited or reordered),
so any release upgrades straight to any later one.

## How a release is made

1. Work lands on `dev` through feature branches, as Conventional Commits.
2. `dev` is merged into `main` by a pull request.
3. On every push to `main`, [release-please](https://github.com/googleapis/release-please)
   keeps a "chore(main): release …" pull request open with the changelog and the version
   bumps: the workspace version in `Cargo.toml` (and `Cargo.lock`, refreshed by the
   workflow), each `package.json`, the OpenAPI documents' `info.version`
   (`.github/workflows/release-please.yml`, `release-please-config.json`,
   `.release-please-manifest.json`).
4. Merging that pull request tags the release and publishes the GitHub Release with the
   changelog; `CHANGELOG.md` feeds [hyphacommerce.com/changelog](https://hyphacommerce.com/changelog).

Images on GHCR and the npm packages are published from the tag by `release.yml`, once it
exists; until then a release is the tag, the changelog and the source.
