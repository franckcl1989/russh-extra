<!--
PR title uses Conventional Commits, e.g.
    feat(client): add buffered command execution
    docs: add native SFTP design
See CONTRIBUTING.md for the full process.
-->

## Summary

<!-- What changed and why? Link the issue it closes. -->

## Design

<!-- Link roadmap entry and design doc. For small changes, explain why no design doc is required. -->

- Roadmap:
- Design:
- Design status: Draft / Accepted / Implemented / Not required

## Type of change

- [ ] Small change: bug fix, docs, internal cleanup, or test
- [ ] Implements a previously accepted design:
- [ ] Roadmap entry + design doc only
- [ ] Other:

## Checklist

- [ ] PR title uses Conventional Commits format
- [ ] `cargo fmt --all` is clean
- [ ] `cargo clippy --workspace --all-features -- -D warnings` is clean
- [ ] `cargo test --workspace --all-features` passes
- [ ] `cargo doc --workspace --all-features --no-deps` passes
- [ ] Relevant `cargo check -p russh-extra --no-default-features ...` feature checks pass
- [ ] Public API behavior is covered by `docs/dev/design/`
- [ ] New SSH/SFTP behavior maps directly to official `russh` APIs
- [ ] Secrets, host keys, paths, and command data are not logged or serialized unexpectedly
- [ ] Security-sensitive or release-ready claims have an independent AI audit note

## Notes for reviewers

<!-- Anything reviewers should focus on. -->
