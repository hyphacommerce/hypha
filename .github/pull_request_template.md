<!-- Thanks! A sentence on what this changes and why; link the issue or discussion it came from. -->

Closes #

## Checks

- [ ] `cargo fmt --all`, `cargo clippy --workspace --all-targets`, `cargo nextest run --workspace`, `cargo test --workspace --doc`
- [ ] `pnpm fmt && pnpm lint && pnpm lint:react && pnpm typecheck` (TypeScript changes)
- [ ] Routes or DTOs changed: `cargo run -p hypha-host-axum -- openapi` and `pnpm sdk:generate`, a step in `scripts/e2e.sh`
- [ ] Incompatible API change: labelled `breaking-api`
- [ ] Commits follow Conventional Commits
