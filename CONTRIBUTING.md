# Contributing to Hypha

Thanks for your interest. Hypha is in public beta: it works end to end, but it's a young
codebase run by one maintainer, so this page says what helps most right now.

## What helps most

- **Bug reports.** Open an issue with the template. The version (`GET /health`, or the
  release tag), what you did, what happened and what you expected. For a crash, the log
  line with `RUST_LOG=info`.
- **Trying it on a real shop** and telling us what was missing or confusing. Use
  [Discussions](https://github.com/hyphacommerce/hypha/discussions) for that.
- **Apps and spores.** The best way to extend Hypha is from outside: an app over HTTP
  (`docs/apps`) or a spore in Rust (`docs/spores`). Both work without a change to this
  repository, and we'll happily list yours.
- **Docs fixes.** Typos, unclear steps, a missing example: a pull request is welcome
  without an issue first.

## Code changes

Open an issue (or a discussion) before a change of any size, so we agree on the shape
before you spend the time; Hypha's crates follow a strict layering (`CLAUDE.md`,
"Architecture") and a change in the wrong layer can't be merged however good it is.
Features go on a branch cut from `dev` and are merged into `dev`; `main` only receives
releases.

Before a change counts as done, these pass (`docker compose up -d` first, for the tests'
Postgres; `cargo install cargo-nextest --locked` once):

```bash
cargo fmt --all
cargo clippy --workspace --all-targets
cargo nextest run --workspace
cargo test --workspace --doc
pnpm fmt && pnpm lint && pnpm lint:react && pnpm typecheck
```

Commits follow [Conventional Commits](https://www.conventionalcommits.org/) (`feat(products): …`,
`fix(checkout): …`): the changelog and the next version are built from them. A pull
request that changes the HTTP API in an incompatible way is labelled `breaking-api`.

## Security

Don't open an issue for a vulnerability. See [SECURITY.md](SECURITY.md).

## License of contributions

Hypha is dual licensed under MIT and Apache-2.0. Unless you say otherwise, any
contribution you intentionally submit for inclusion in the work, as defined in the
Apache-2.0 license, is licensed the same way, without additional terms. There is no
separate contributor agreement to sign.
