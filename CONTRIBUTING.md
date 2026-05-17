# Contributing to russh-extra

`russh-extra` is developed through design documents, tests, and focused
implementation changes. The project is AI-agent driven, and the repository
remains the source of truth.

Before making substantial changes, read:

- `docs/dev/project-charter.md`
- `docs/dev/constraints.md`
- `docs/dev/development-plan.md`
- `docs/dev/roadmap.md`
- Relevant files under `docs/dev/design/`

## Two Paths

### Small Changes

Open a PR directly for bug fixes, docs corrections, internal cleanup, or tests
that do not change public API behavior.

### Public API Changes

For new features, public API changes, protocol behavior, or handler contracts:

1. Add or update an entry in `docs/dev/roadmap.md`.
2. Add a guide-level design document under `docs/dev/design/`.
3. Mark the design Accepted only after public API, behavior, errors, security,
   feature flags, and tests are specified.
4. Implement the feature after the design is accepted.

The design doc should explain what users call, what behavior they observe, what
errors they handle, and how the feature maps to `russh`.

After implementation, before marking a design Implemented, completing a release,
or declaring security-sensitive behavior production-ready, an independent AI
audit must be performed and recorded in `docs/dev/audits/`. See
`docs/dev/decisions/0002-ai-only-no-human-gate-independent-audit.md` and
`docs/dev/ai-workflow.md` for the audit process.

## Scope

This crate builds on the official `russh` crate. Do not add third-party SSH,
SFTP, shell, tunnel, or protocol helper crates. If `russh` lacks a primitive
needed by the high-level API, document the gap and either implement the layer in
this repository over public `russh` APIs or track the upstream `russh` change.
See `docs/dev/constraints.md` for the full dependency policy.

## Before Submitting

```bash
just check-all  # recommended: runs the full suite
```

Or run commands individually:

```bash
cargo check --workspace --all-targets --all-features
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo check -p russh-extra --no-default-features
cargo check -p russh-extra --no-default-features --features client,aws-lc-rs
cargo check -p russh-extra --no-default-features --features server,aws-lc-rs
cargo check -p russh-extra --no-default-features --features known-hosts,aws-lc-rs
cargo check -p russh-extra --no-default-features --features shell,aws-lc-rs
cargo check -p russh-extra --no-default-features --features tunnel,aws-lc-rs
cargo check -p russh-extra --no-default-features --features sftp,aws-lc-rs
cargo check -p russh-extra --no-default-features --features server,sftp,aws-lc-rs
cargo check -p russh-extra --no-default-features --features client,ring
cargo check -p russh-extra --no-default-features --features full
cargo doc --workspace --all-features --no-deps
```

## Commit and PR Titles

Use Conventional Commits:

```text
type: description
```

Allowed types are `feat`, `fix`, `docs`, `style`, `refactor`, `perf`, `test`,
`build`, `ci`, `chore`, and `revert`.

See `docs/dev/COMMITS.md` for examples.
