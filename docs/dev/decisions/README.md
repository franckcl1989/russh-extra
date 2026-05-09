# Architecture Decision Records (ADRs)

This directory records durable architectural decisions for `russh-extra`.
Each ADR captures the context, decision, rationale, and consequences of a
choice that affects the project's architecture, API design, feature gates,
dependency policy, or operating model.

## When to Write an ADR

Write an ADR when a decision:

- Affects multiple features or modules.
- Constrains future design choices.
- Affects the public API compatibility surface.
- Changes the feature flag model or dependency policy.
- Sets a project-level policy that future agents must follow.

Do not write an ADR for routine implementation details that can be cleanly
understood from code, design docs, or conventional commit messages.

## Format

Each ADR is a standalone markdown file with a numbered prefix and descriptive
slug:

```text
NNNN-slug.md
```

Start from `../design/_template.md` for structure, and include at minimum:
context, decision, rationale, consequences, and alternatives considered.

Status values follow the same convention as design docs: Draft, Accepted,
Superseded.

## Index

| Number | Title | Status |
|--------|-------|--------|
| 0001 | [SFTP Marker Types and `full` Exclusion](./0001-sftp-marker-types-and-full-exclusion.md) | Superseded |
| 0002 | [AI-Only Operating Model: No Human Gate, Independent Audit](./0002-ai-only-no-human-gate-independent-audit.md) | Accepted |
| 0003 | [Feature Flag Naming Conventions](./0003-feature-flag-naming.md) | Accepted |
