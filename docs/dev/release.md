# Release and Compatibility Policy

This project is in pre-release development. Public API compatibility is not
promised until the project declares a stable release policy.

## Versioning

The workspace currently uses `0.1.1`. Before `1.0`, public APIs may change when
design docs require it or when a cleaner architecture is needed. Breaking
changes are expected during this phase and should update design docs, roadmap
status, tests, and examples in the same work item.

After a stable compatibility policy is accepted, releases should follow
semantic versioning:

- Patch releases fix bugs without changing public API behavior.
- Minor releases add compatible public API.
- Major releases allow breaking public API changes.

## Public API Stability

An API is not considered stable until:

- Its design doc is Accepted or Implemented.
- Public examples are tested or explicitly marked illustrative.
- Feature-gating behavior is documented and checked.
- Error behavior and security behavior are specified.
- Integration tests cover the primary runtime path.
- An independent AI audit has reviewed the API claims, security behavior,
  feature gates, and test coverage from repository files alone.

Draft designs may contain illustrative APIs. Do not treat them as compatibility
promises.

Pre-1.0 compatibility must not block replacing weak scaffolding with a better
API that serves the project goal.

## Feature Flags

Feature flags are part of the public contract. Changes to feature dependencies
must update:

- `Cargo.toml`
- `.github/workflows/ci.yml`
- `AGENTS.md`
- `CONTRIBUTING.md`
- `docs/dev/testing.md`
- Relevant design docs

`russh-extra --no-default-features` must keep compiling.

## MSRV

The workspace MSRV is declared in root `Cargo.toml` and CI. Any MSRV change
must update:

- Root `Cargo.toml`
- `.github/workflows/ci.yml`
- Release notes

Do not raise MSRV as part of unrelated feature work.

## Release Checklist

Before publishing a release:

- [ ] Roadmap statuses match implementation state.
- [ ] Implemented features have Implemented design docs.
- [ ] `cargo fmt --all --check` passes.
- [ ] `cargo clippy --workspace --all-features -- -D warnings` passes.
- [ ] `cargo test --workspace --all-features` passes.
- [ ] `cargo doc --workspace --all-features --no-deps` passes.
- [ ] Each publishable package is checked individually with `cargo package`.
      The workspace package command is not a release gate because
      `russh-extra-test-support` and `russh-extra-tests` are `publish = false`.
- [ ] Feature-gating checks from `docs/dev/testing.md` pass.
- [ ] README examples match implemented behavior.
- [ ] README and crate documentation examples are compiled or explicitly marked
      illustrative, `no_run`, or ignored with a reason.
- [ ] An independent AI audit note records blocking findings, resolved findings,
      and residual risks.
- [ ] Security-sensitive behavior is documented.
- [ ] Release notes list public API changes and known limitations.

## Publishing Order

Publish packages in dependency order:

```bash
cargo package -p russh-extra-core
cargo publish -p russh-extra-core
cargo package -p russh-extra
cargo publish -p russh-extra
```

`russh-extra` depends on `russh-extra-core = "0.1.1"`, so the core crate must
be available in the crates.io index before packaging or publishing the
user-facing crate without a path override.
