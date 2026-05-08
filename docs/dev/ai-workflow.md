# AI Development Workflow

`russh-extra` is structured so AI agents can move the project forward without
depending on hidden context. This file defines the default prompts, handoff
rules, and completion checks agents should use.

## Default Agent Prompt

Use this prompt when starting a new Codex session for general project work:

```text
You are working in russh-extra, an AI-driven Rust workspace that builds a
complete high-level async SSH API directly on official russh APIs. Read
AGENTS.md, docs/dev/project-charter.md, docs/dev/constraints.md,
docs/dev/development-plan.md, docs/dev/security.md, docs/dev/roadmap.md, and
any relevant design docs before editing.

Do not add third-party SSH, SFTP, shell, tunnel, or protocol helper crates.
For non-trivial public API work, update the roadmap and design docs before
implementation. Keep durable decisions in repository files, not chat. Use
integration tests with local russh fixtures for public behavior and run
cargo fmt --all after code changes. This is a pre-1.0 AI-only project: make
breaking changes when they improve the architecture, and do not wait for human
review gates.
```

## Feature Design Prompt

Use this prompt to draft or revise a public API design:

```text
Design the next russh-extra slice for <feature>. Start from
docs/dev/design/_template.md. The design must explain the user-facing API,
runtime behavior, typed errors, cancellation and shutdown, security behavior,
feature gates, compatibility, mapping to official russh APIs, tests, open
questions, and out-of-scope work.

Do not mark the design Accepted while blocking questions remain. If russh lacks
a needed primitive, document the gap and prefer a local layer over public russh
APIs or an upstream russh issue.
```

## Implementation Prompt

Use this prompt after a relevant design is Accepted:

```text
Implement the accepted russh-extra design in <design-doc>. Keep changes scoped
to the roadmap item. Preserve existing public API behavior unless the design
explicitly changes it. Add or update tests listed in the design. Do not add
third-party SSH protocol helper crates. Run cargo fmt --all, relevant feature
checks, and relevant tests before finishing.
```

## Test Development Prompt

Use this prompt when adding test fixtures or coverage:

```text
Add russh-extra tests for <behavior>. Prefer integration coverage for public
API behavior. Use russh-extra-test-support loopback fixtures for SSH networking
and do not depend on external hosts. Include happy path, typed error paths,
feature-gating behavior when relevant, cancellation or disconnect behavior when
the design calls it out, and regression cases for protocol ordering.
```

## Review Prompt

Use this prompt for code review:

```text
Review the current russh-extra change for correctness, public API behavior,
design alignment, missing tests, feature-gate regressions, security issues, and
accidental dependency-policy violations. Lead with findings and cite file and
line references. Check whether durable decisions belong in docs before merge.
```

## Independent Audit Prompt

Use this prompt before marking a design Implemented, completing the release
checklist, or declaring security-sensitive behavior production-ready:

```text
Audit the current russh-extra repository from files alone. Do not assume prior
chat context. Check prompt compliance, design status accuracy, README and
crate-doc claims, feature-gate behavior, security defaults, dependency policy,
test coverage, and release readiness. Lead with findings and cite file and line
references. If no blocking findings remain, state the remaining risks and where
they are tracked.
```

The audit should be performed by a separate AI pass from the implementation pass
when practical. Any durable decision or residual risk discovered by the audit
must be recorded in `docs/dev/audits/` using the template at
`docs/dev/audits/_template.md`, not only in chat.

## Document Drift Check

Several documents carry Status fields, feature flag lists, and capability
claims that must stay consistent. When changing any of the following, check
and update all others that reference the same information:

| Change trigger | Must also check |
|---|---|
| Design doc Status or feature list | `docs/dev/design/README.md`, `docs/dev/roadmap.md` |
| Feature flag added, renamed, or removed | `crates/russh-extra/Cargo.toml`, `AGENTS.md` (Feature Flags section), `README.md` (Feature Flags section), `CHANGELOG.md`, `docs/dev/roadmap.md`, `docs/dev/testing.md`, `.github/workflows/ci.yml` |
| `full` feature set changed | `AGENTS.md`, `README.md`, `CHANGELOG.md`, `docs/dev/roadmap.md`, `docs/dev/design/README.md` |
| README Current Status updated | `docs/dev/roadmap.md`, `CHANGELOG.md` |
| Roadmap status changed | `docs/dev/design/README.md`, `README.md` |
| CI feature-gating checks changed | `docs/dev/testing.md`, `AGENTS.md` (Verification Commands), `README.md` (Feature Flags section) |
| AGENTS.md verification commands changed | `.github/workflows/ci.yml`, `README.md` (Development section), `CONTRIBUTING.md` |
| Design marked Implemented | `docs/dev/audits/` (create audit record) |

The repository is the memory. If one file claims a feature is Implemented and
another lists it as Draft, the next agent will not know which to trust. Always
leave all files consistent.

Git history is the authoritative record of when a file was last modified.
Documents do not carry manual `Last updated` dates. Use `git log` to determine
freshness.

## Handoff Notes

Before ending a substantial session, leave the repository in a state where the
next agent can continue from files alone:

- Update roadmap status and design docs when decisions changed.
- Keep Draft designs Draft when questions remain.
- Record implementation blockers in the relevant design doc.
- Run the relevant commands or state why they were not run.
- Do not rely on chat context for unfinished decisions.
- Do not defer decisions only because a human has not reviewed them.
- Do not mark a security-sensitive feature Implemented or release-ready without
  an independent AI audit note recorded in repository files.

## Completion Checklist

For documentation-only work:

- [ ] Links from `docs/dev/README.md` are current.
- [ ] Roadmap status matches the changed docs.
- [ ] All documents listed in the Document Drift Check table are consistent
      with the change.
- [ ] No durable decision exists only in the final chat response.
- [ ] Audit records and ADRs are indexed in their respective README files.

For code work:

- [ ] `cargo fmt --all` was run.
- [ ] Relevant tests were run.
- [ ] Relevant feature-gate checks were run.
- [ ] Public API work cites an Accepted design.
- [ ] New errors and security behavior are documented.
- [ ] Implemented/release-ready/security-sensitive changes have an independent
      AI audit note or an explicit follow-up item.
