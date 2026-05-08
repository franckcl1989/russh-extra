# Development Plan

This file defines when `russh-extra` can move from project scaffolding into
runtime implementation.

## Current Verdict

The repository has completed the first client and server runtime slices.
`Client::connect()`, buffered `Session::command()`, known-hosts integration,
public-key authentication, keyboard-interactive authentication, shell and
subsystem channels, streaming server exec, and the first TCP forwarding runtime
are present as public pre-1.0 APIs.

The next gate is hardening: keep docs, examples, tests, and feature-gating
checks aligned with the actual behavior, and avoid expanding unfinished APIs
before the relevant design is accepted.

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

- Native SFTP packet runtime and high-level file APIs.
- Streamlocal forwarding runtime.
- Dynamic SOCKS-style forwarding runtime.
- Split shell/tunnel read/write halves and `AsyncRead`/`AsyncWrite` trait impls
  for high-level handles.
- New public escape hatches to lower-level `russh` handles outside the
  implemented client raw-handle guard and accepted server host-key/context
  handles.

Experimental code may be used to inspect `russh` behavior, but it should not be
presented as final public API until the design gate is satisfied.

Agents do not wait for human review gates between phases. A phase advances when
the repository docs, code, and verification results satisfy the gate.

## Phase Gates

### Phase 0: Repository Foundation

Status: Implementing.

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

- Server API design is Implementing and aligned with the implemented runtime
  slices.
- Server can authenticate, route commands, reject unauthorized requests, and
  shut down predictably.
- Client and server loopback tests cover both sides.

### Phase 5: Shells, SFTP, and Tunnels

Status: Implementing.

Done when:

- Channel/shell and forwarding designs match the implemented runtime and test
  coverage.
- Native SFTP has an Accepted design before file-operation runtime work starts.
- Each implemented feature has local integration tests, negative tests, and
  feature-gating checks.

## Immediate Backlog

1. Add more forwarding lifecycle tests, especially remote forwarding cancel and
   error paths.
2. Decide whether to implement native SFTP packet framing or keep the `sftp`
   feature reserved with marker types for the next release.
3. Expand known-hosts tests for file save/load, permission errors, revoked
   entries, and malformed lines.
