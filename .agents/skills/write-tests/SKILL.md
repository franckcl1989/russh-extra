---
name: write-tests
description: Use before writing or editing tests in the russh-extra repository
---

# Writing russh-extra Tests

Prefer integration tests for public API behavior.

## Where Tests Go

| What is tested | Location |
|---|---|
| Public API behavior | `tests/tests/` |
| Shared core parsing or validation | `crates/russh-extra-core/tests/` or inline unit tests |
| Local SSH networking behavior | `tests/tests/` with `russh-extra-test-support` |
| Macro behavior | user-level compile or integration tests |

## Rules

- Keep each test focused on one behavior.
- Use helpers from `russh-extra-test-support` for tracing and local fixtures.
- Do not test macro internals directly.
- For protocol encoders and decoders, include malformed and boundary cases.
- Network tests must use local loopback servers, not external SSH hosts.
- Public API implementation must include at least one user-level integration
  test unless the change is purely internal.
- Feature-gated APIs must be checked with `--no-default-features` combinations
  that match `.github/workflows/ci.yml`.
- Tests that use secrets or host keys must generate local fixtures and avoid
  committing private keys.
- Error-path tests should assert typed errors, not only string messages.
