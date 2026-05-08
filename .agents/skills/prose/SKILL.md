---
name: prose
description: Use when writing or editing russh-extra documentation, design docs, READMEs, issues, PRs, or commit bodies
---

# Writing russh-extra Prose

Write direct, factual documentation.

## Style

- State what the API does and when to use it.
- Prefer active voice and present tense.
- Use concrete examples instead of broad claims.
- Avoid buzzwords, hype, and vague adjectives.
- Do not describe historical chat decisions. Put durable decisions in docs.
- Call out protocol boundaries precisely.

When discussing SFTP, forwarding, shells, or SSH protocol behavior, make clear
that `russh-extra` builds directly on official `russh` APIs.

## Dates and Freshness

Do not add manual `Last updated` dates to documents. Git history is the
authoritative record of when a file was last modified. Use `git log` to
determine freshness. Manual dates are prone to drift and create false
freshness when agents forget to update them.
