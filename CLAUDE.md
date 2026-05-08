# CLAUDE.md

This file provides guidance to Claude Code when working in this repository.
Codex uses the same rules from `AGENTS.md`; keep the two files aligned.

## Commands

```bash
cargo build --workspace --all-features
cargo test --workspace --all-features
cargo clippy --workspace --all-features -- -D warnings
cargo fmt --all
cargo check -p russh-extra --no-default-features
cargo check -p russh-extra --no-default-features --features client,aws-lc-rs
cargo check -p russh-extra --no-default-features --features server,aws-lc-rs
cargo check -p russh-extra --no-default-features --features known-hosts,aws-lc-rs
cargo check -p russh-extra --no-default-features --features shell,aws-lc-rs
cargo check -p russh-extra --no-default-features --features tunnel,aws-lc-rs
cargo check -p russh-extra --no-default-features --features sftp,aws-lc-rs
cargo check -p russh-extra --no-default-features --features client,ring
cargo doc --workspace --all-features --no-deps
```

## Workflow

Always run `cargo fmt --all` after finishing work on a change.

Public API work should be design-led:

1. Add or update `docs/dev/roadmap.md`.
2. Add a guide-level design document under `docs/dev/design/`.
3. Mark the design Accepted only after public API, behavior, errors, security,
   feature flags, and tests are specified.
4. Implement after the design is accepted.

Small fixes, docs corrections, and internal cleanup can go straight to code.
Do not implement non-trivial public API from a Draft design.

Before marking a design Implemented, completing a release checklist, or
declaring security-sensitive behavior production-ready, run an independent AI
audit from repository files alone and record the findings or residual risks in
repository docs.

## Architecture

`russh-extra` is a high-level async SSH API for Rust built on top of `russh`.
It is a Cargo workspace.

| Crate | Purpose |
|---|---|
| `russh-extra` | User-facing API: clients, servers, shells, SFTP, tunnels, and re-exports |
| `russh-extra-core` | Shared SSH domain types and errors |
| `russh-extra-macros` | Future proc-macro entry points |
| `russh-extra-test-support` | Integration test helpers |
| `russh-extra-tests` | Workspace-level API and integration tests |

High-level APIs should hide packet and channel bookkeeping, but not hide SSH
semantics. Users should be able to drop down to `russh` types when needed.

### Further Reading

- `AGENTS.md` — permanent AI agent project instructions
- `.agents/skills/` — shared skill files (canonical location for all agent runtimes)
- `docs/dev/project-charter.md`
- `docs/dev/constraints.md`
- `docs/dev/ai-workflow.md`
- `docs/dev/testing.md`
- `docs/dev/development-plan.md`
- `docs/dev/security.md`
- `docs/dev/release.md`
- `docs/dev/architecture/README.md`
- `docs/dev/design/_template.md`
- `docs/dev/roadmap.md`
