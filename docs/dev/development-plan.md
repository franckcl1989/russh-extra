# Development Plan

This file defines when `russh-extra` can move from project scaffolding into
runtime implementation.

## Current Verdict

The repository has completed the 0.1.0 release (first client and server runtime
slices), the 0.1.1 hardening release (documentation governance, SFTP error
mapping, StreamLocal lifecycle hardening, known-hosts edge cases), the 0.1.2
hardening release (security fixes, data-loss bugs, API contract fixes, small
missing features, debug redaction tests), the 0.1.3 hardening release (test
hardening, API completions, documentation drift fixes, dependency update),
and the 0.1.4 hardening release (Windows CI fix, timeout wiring, SFTP error
quality, base64 data integrity, tracing coverage, 284 tests).
The 0.1.5 release fixes a Windows CI dead-code regression in the StreamLocal
forwarding compiler attributes.

The next gate is `0.2`: deferred features including hashed and wildcard
known-hosts matching, dynamic SOCKS forwarding, SFTP v4+ extensions, and
split read/write halves for shell and tunnel handles.

## Work Allowed Now

Agents may work on:

- Hardening implemented client APIs: connect, auth, known-hosts, buffered
  command execution, shell/subsystem channels, and TCP forwarding.
- Hardening implemented server APIs: listener startup, host keys,
  password/public-key/keyboard-interactive authentication, buffered and
  streaming exec routing, shell/PTY/subsystem hooks, forwarding authorization,
  lifecycle hooks, and shutdown.
- Extending `russh-extra-test-support` loopback fixtures needed by accepted
  designs.
- Adding unit tests for feature-neutral core types.
- Adding API smoke tests that match existing public builders and domain types.
- Improving CI, feature-gating checks, docs, prompts, constraints, and design
  documents.
- Refactoring internal scaffolding when public behavior does not change.
- Breaking or replacing pre-1.0 APIs when doing so improves the full-featured
  `russh`-based design.

## Work Blocked Until Design Acceptance

Agents must not implement these as durable public runtime APIs while the
relevant design remains Draft:

- Dynamic SOCKS-style forwarding runtime.
- Split shell/tunnel read/write halves and direct `AsyncRead`/`AsyncWrite`
  trait impls for high-level handles beyond the implemented `ShellAsyncIo`
  wrapper.
- New public escape hatches to lower-level `russh` handles outside the
  implemented client raw-handle guard and accepted server host-key/context
  handles.

Experimental code may be used to inspect `russh` behavior, but it should not be
presented as final public API until the design gate is satisfied.

Agents do not wait for human review gates between phases. A phase advances when
the repository docs, code, and verification results satisfy the gate.

## Phase Gates

### Phase 0: Repository Foundation

Status: Completed.

Done when:

- Workspace crates compile.
- CI covers format, check, clippy, tests, MSRV, and feature gates.
- `AGENTS.md`, `CLAUDE.md`, project charter, constraints, AI workflow, testing
  strategy, security policy, release policy, roadmap, and design index exist.
- All non-trivial public API areas have Draft design docs.

### Phase 1: First Accepted Runtime Slice

Status: Completed.

Done when:

- Error taxonomy design is Implemented.
- Client session design is Accepted for connect and buffered command execution.
- The channel event model needed for buffered command execution is accepted in
  `docs/dev/design/client-session-api.md`.
- Host-key verification, credential order, timeout policy, bounded buffering,
  and `russh` handle ownership are specified.
- Loopback fixture requirements are specified.

### Phase 2: Loopback Test Fixtures

Status: Completed for the first client runtime slice.

Done when:

- `russh-extra-test-support` can start local `russh` servers on ephemeral
  loopback addresses.
- Fixtures can model auth success, auth failure, host-key verification,
  command success, non-zero exit, missing status, stderr, and disconnect.
- Tests do not depend on user SSH agents, local SSH config, external hosts, or
  public network access.

### Phase 3: Client MVP

Status: Completed for the first client runtime slice.

Done when:

- `Client::connect()` works against loopback fixtures.
- Buffered `Session::command()` returns typed stdout, stderr, and exit details.
- Host-key rejection, auth rejection, timeout, channel rejection, and disconnect
  paths return typed errors.
- Feature-gating checks pass.

### Phase 4: Server MVP

Status: Completed for the first server runtime slice.

Done when:

- Server API design is Implemented (first runtime slice) and aligned with the
  implemented runtime slices.
- Server can authenticate, route commands, reject unauthorized requests, and
  shut down predictably.
- Client and server loopback tests cover both sides.

### Phase 5: Shells, SFTP, and Tunnels

Status: Implemented for the 0.1 line scope.

Done when:

- Channel/shell and forwarding designs are Implemented (first runtime slice)
  and match the implemented runtime and test coverage.
- Native SFTP has an Implemented design and runtime coverage for client and
  server handler paths.
- Each implemented feature has local integration tests, negative tests, and
  feature-gating checks.

## Immediate Backlog

The version-specific `0.1.4` scope is completed. `0.1.4` is the final release
in the `0.1.x` series. Remaining work is tracked in the deferred items in
[`roadmap.md`](roadmap.md).

Completed in 0.1.1: documentation reconciliation and governance drift fix,
forwarding lifecycle tests (StreamLocal close/abort), known-hosts edge-case
tests, SFTP server handler error-to-status-code mapping.

Completed in 0.1.2: security leaks (X11 cookies, keyboard-interactive
responses), data-loss bugs (ShellAsyncIo stderr drop, KnownHosts::check
short-circuit), API contract fixes (Credential::PartialEq, Endpoint::Display,
#[non_exhaustive] markers), small missing features (CommandOutput::check_success,
tilde expansion, TerminalMode variants, comma-separated known-hosts parsing),
debug redaction tests.

Deferred beyond `0.1`: large data tests, ShellAsyncIo lifecycle tests,
remaining SftpServerHandler method coverage, hashed known-hosts matching/writing,
wildcard known-hosts matching, dynamic SOCKS-style forwarding, SFTP v4+
extensions.
