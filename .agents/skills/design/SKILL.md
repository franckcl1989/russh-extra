---
name: design
description: Use before authoring or editing a russh-extra design document under docs/dev/design/
---

# Authoring russh-extra Design Documents

Always read `docs/dev/design/_template.md` before writing or editing a design
document.

Design docs are guide-level contracts. Write for users of `russh-extra` and for
people implementing the feature in this repository.

## Before Editing

- Read `docs/dev/roadmap.md`, `docs/dev/architecture/README.md`, and any related
  design document.
- Check whether the roadmap item is Draft, Accepted, Implementing, or
  Implemented.
- Do not implement non-trivial public API from a Draft design.
- If code already exists, reconcile the design with the code or call out the
  mismatch.

## Rules

- Lead with the user-visible API and behavior.
- Explain how the feature maps to official `russh` concepts.
- Do not add third-party SSH, SFTP, shell, tunnel, or protocol helper crates.
- If `russh` lacks a needed primitive, document the gap in "Mapping to russh"
  and prefer a local layer over public `russh` APIs or an upstream `russh`
  change.
- Keep implementation module plans out unless they affect public behavior.
- Use concrete Rust examples.
- Resolve blocking open questions before marking a design Accepted.
- Include feature flags, security behavior, and a test plan.

## Workflow

Non-trivial public API work needs both:

- a roadmap entry in `docs/dev/roadmap.md`
- a design doc in `docs/dev/design/`

Implementation work should cite the accepted design section it follows. If the
implementation discovers a `russh` API limitation, update the design before
continuing.
