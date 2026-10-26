---
title: Releases and upgrades
description: What a Hypha version promises, what the beta means, and how to upgrade.
---

Hypha ships as one set: the API, the admin, the storefront, the CLI and the npm packages
come from the same commit and are only right together. So there is **one version and one
tag per release**, and a release publishes:

- the source, tagged on [GitHub](https://github.com/hyphacommerce/hypha/releases);
- the images `ghcr.io/hyphacommerce/hypha-api`, `hypha-admin` and `hypha-storefront`, for
  amd64 and arm64, tagged with the version;
- the npm packages under `@hyphacommerce/`, with provenance;
- the API's Cloudflare Workers bundle, attached to the release.

The API reports its version in its OpenAPI documents and to MCP clients.

## The beta

Hypha is in public beta, `0.1.0-beta.N`. It works end to end and the
[demo](https://demo.hyphacommerce.com) runs it, but it hasn't carried a real shop's orders
yet. During the beta:

- any release may change the HTTP API, event payloads, extension points or the
  configuration keys, and each change is in the [changelog](/changelog);
- the packages and images are on the `next` tag, so install and deploy with `@next`;
- migrations are still append-only, so a database made by one beta upgrades to the next;
- fixes land in the next release, there are no backports.

The beta ends with `0.1.0`, once a few shops have run on it.

## Versions after the beta

SemVer, read the 0.x way: the **minor** number changes for incompatible changes
(`0.1` → `0.2`), the **patch** number for fixes and additions. Covered by that promise:

- the HTTP API, held by an automated compatibility check on every change;
- event payloads and extension points, each versioned on its own as well;
- the app manifest schema and the block format;
- `hypha.config.toml`'s keys, which are never renamed;
- the npm packages' exports.

Not yet: the Rust spore API (`hypha-spore`), until the crates are published. Spores depend
on a git tag and may need changes between releases.

## Upgrading

A deployment made by the CLI upgrades by running the newer CLI's `deploy`: it pulls that
version's images, and the API runs its migrations on start. Any release upgrades straight
to any later one. Read the changelog first for changes to the API or the configuration
that affect your own frontend or apps.
