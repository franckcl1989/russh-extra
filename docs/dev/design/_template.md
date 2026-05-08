# {Feature name}

Status: Draft
Roadmap: `docs/dev/roadmap.md#{section}`

<!--
Design documents are implementation contracts. Keep every section. If a section
does not apply, write "Not applicable" and explain why.

Status values:
- Draft: open questions remain; do not implement public API from this document.
- Accepted: public API and behavior are stable enough to implement.
- Implementing: code is in progress and must stay aligned with this document.
- Implemented: code and tests match this document.
-->

## Summary

State what changes for users and why it belongs in `russh-extra`.

## Motivation

The concrete SSH workflows this solves. Include examples of code that is
awkward, repetitive, or error-prone when written directly against `russh`.

## Accepted Decisions

List decisions that implementation must follow:

- Public API shape:
- Error policy:
- Cancellation and shutdown policy:
- Feature flags:
- Escape hatches to `russh`:

## User-facing API

Write this section like a user guide. Show idiomatic Rust examples. Explain
what to call and when to use it.

## Behavior

Describe runtime behavior:

- Happy path.
- Error cases and error types.
- Defaults.
- Cancellation and shutdown behavior.
- Interactions with authentication, sessions, channels, and forwarding.

## Security

Describe host-key handling, secret handling, local file permissions, logging
redaction, and any safe defaults. If the feature does not touch security-sensitive
data, say so.

## Mapping to `russh`

Explain which `russh` concepts this feature uses:

- Client or server handlers.
- Session channels.
- Channel requests.
- Global requests.
- Subsystems.
- Access to lower-level handles.

If a feature cannot be implemented cleanly with current `russh` APIs, state the
gap explicitly. The accepted path is a local layer over public `russh` APIs or
an upstream `russh` change, not another SSH protocol crate.

## Feature Flags and Compatibility

State which crate features expose the API, what compiles with
`--no-default-features`, and whether the change is public API compatible.

## Edge cases

Cases that affect user code and are easy to get wrong:

- Boundary values.
- Concurrent channel use.
- EOF, exit status, and signal ordering.
- Backpressure.
- Remote disconnects.
- Platform differences.

## Testing Plan

List the tests required before implementation is complete:

- Unit tests:
- Integration tests:
- Feature-gating checks:
- Local networking fixtures:
- Negative/error-path tests:

## Alternatives considered

Other designs considered and why they were not chosen.

## Open questions

Open decisions. Mark each as blocking acceptance, blocking implementation, or
deferrable.

## Out of scope

Related work that this design does not cover.

## Acceptance Checklist

- [ ] User-facing API examples compile or are marked as illustrative.
- [ ] Runtime behavior and error policy are fully specified.
- [ ] Mapping to official `russh` APIs is explicit.
- [ ] Security-sensitive data handling is specified.
- [ ] Feature flags and no-default behavior are specified.
- [ ] Tests required for implementation are listed.
- [ ] Open questions are either resolved or marked deferrable.
