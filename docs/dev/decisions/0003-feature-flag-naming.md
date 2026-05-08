# ADR 0003: Feature Flag Naming Conventions

Status: Accepted

## Context

`russh-extra` exposes a growing set of Cargo feature flags that control
functional capabilities, cryptographic backends, and serialization support.
Without a naming convention, feature flag names could become inconsistent
or misleading as the crate matures.

## Decision

1. **Functional features** use lowercase, hyphenated names that describe the
   capability: `client`, `server`, `shell`, `tunnel`, `known-hosts`, `sftp`,
   `agent`.
2. **Cryptographic backend features** use the crate name of the backend
   directly: `aws-lc-rs`, `ring`. These are delegated to `russh`'s
   corresponding features.
3. **Serialization features** use the crate name: `serde`.
4. **Internal features** (not user-facing) are prefixed with underscore:
   `_russh`.
5. **`full`** enables all stable runtime functionality. It must not include
   features that expose only reserved or experimental marker types.
6. **`default`** enables a conservative, secure subset: `client`,
   `known-hosts`, and `aws-lc-rs`.

## Rationale

- **Consistency with `russh`**: Cryptographic backend features match the
  upstream `russh` feature names, avoiding confusion.
- **Discoverability**: Functional feature names directly describe what the
  user gets (`shell` gives shell support).
- **`full` integrity**: `full` must be truthful. Including features with no
  runtime would mislead users.
- **Conservative defaults**: `default` enables the most common secure
  configuration (client + host key verification) without pulling in server or
  tunneling.

## Consequences

- New functional features must follow the lowercase-hyphenated convention.
- Before adding a feature to `full`, the feature must have real runtime
  behavior.
- Changing a feature flag name is a breaking change and must be documented in
  CHANGELOG.
- The `full` and `default` sets are documented in README and crate docs and
  must stay synchronized.

## Alternatives Considered

- **`snake_case` feature names**: Rejected because Cargo convention uses
  hyphens in feature names (`known-hosts` not `known_hosts`).
- **Grouped prefixes** (`ssh-client`, `ssh-server`): Rejected as unnecessary
  verbosity; the crate name already implies SSH.
- **Include `sftp` in `full` with marker types**: Rejected (see ADR 0001).
