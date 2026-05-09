# Project Charter

`russh-extra` is an AI-driven Rust workspace for building a broad high-level
async SSH API directly on top of the official `russh` crate.

The repository is the project memory. Goals, design decisions, constraints,
test requirements, and implementation status must be captured in files in this
repository instead of relying on chat history.

## Goal

Build a full-featured SSH API layer for common SSH workflows while preserving
the SSH concepts exposed by `russh`. The high-level API is not expected to hide
or mirror every low-level `russh` hook; unsupported advanced workflows should
remain reachable through explicit escape hatches.

The public API should cover:

- Client connection, authentication, host-key verification, and session
  management.
- Remote command execution with buffered and streaming output modes.
- Interactive shells, PTYs, terminal modes, resize, environment variables, and
  signals.
- Subsystem channels, including a native SFTP implementation.
- Server authentication, session handlers, command routing, shells, subsystem
  routing, and forwarding handlers.
- Local, remote, direct, and streamlocal forwarding where supported by `russh`.
- Typed channel wrappers with `AsyncRead`, `AsyncWrite`, EOF, exit-status, and
  signal behavior.
- Escape hatches to lower-level `russh` handles when the high-level API does
  not cover a workflow.

## Operating Model

The operating model is AI-only development. Codex agents own planning, design,
implementation, tests, review notes, issues, PR text, commit text, release
notes, and maintenance docs.

The project does not require human implementation, human review, or human
approval gates to move forward. Goal-setting prompts are treated as input to
the Codex workflow, not as durable project governance. Durable technical
decisions must live in the repository.

AI-only development does not mean single-pass self-approval. Before a design is
marked Implemented, before a release checklist is completed, or before a
security-sensitive behavior is declared production-ready, an independent AI
review pass must audit the change from repository files alone. That review must
focus on prompt compliance, design alignment, security behavior, test coverage,
feature-gating, and documentation claims. Findings and any residual risk must be
recorded in the relevant design, roadmap, release notes, or follow-up issue so
the next agent can continue without chat context.

Every agent should work from this source order:

1. `AGENTS.md` or the equivalent tool-specific instruction file.
2. `docs/dev/project-charter.md`.
3. `docs/dev/constraints.md`.
4. `docs/dev/development-plan.md`.
5. `docs/dev/security.md`.
6. `docs/dev/roadmap.md`.
7. Relevant design docs under `docs/dev/design/`.
8. The code and tests.

## Success Criteria

The project is ready for broad use when:

- The main client, server, shell, SFTP, and forwarding workflows are covered by
  accepted design docs, implementation, and integration tests.
- Users can write common workflows without managing raw SSH channel
  bookkeeping.
- Advanced users can access lower-level `russh` handles without replacing the
  connection stack.
- Tests run without external SSH hosts.
- Feature gates compile independently.
- Public behavior, errors, security decisions, and compatibility promises are
  documented before release.
- Codex can continue development from repository files alone.

## Non-goals

The project does not:

- Wrap third-party SSH, SFTP, shell, tunnel, or protocol abstraction crates.
- Hide SSH semantics behind a generic remote-execution abstraction.
- Depend on external SSH hosts for tests.
- Add macro-only runtime behavior that cannot be understood through normal
  Rust APIs.
- Treat chat transcripts as project documentation.
