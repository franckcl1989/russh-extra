# Git Commit Guidelines

This repository follows Conventional Commits.

## Format

```text
<type>(<scope>): <subject>

<body>

<footer>
```

Scopes are optional. Keep lines under 100 characters.

## Types

- `feat`: a new feature
- `fix`: a bug fix
- `docs`: documentation only changes
- `style`: formatting-only changes
- `refactor`: code change that neither fixes a bug nor adds a feature
- `perf`: performance improvement
- `test`: adding or correcting tests
- `build`: build system or dependency changes
- `ci`: CI configuration changes
- `chore`: auxiliary tooling
- `revert`: revert a previous commit

## Common Scopes

- `core`: `russh-extra-core`
- `client`: client API
- `server`: server API
- `sftp`: native SFTP layer
- `tunnel`: forwarding and tunnel APIs
- `macros`: `russh-extra-macros`
- `tests`: integration test suite
- `docs`: documentation when `type` is not already `docs`

## Subject

Use imperative, present tense. Start with a lowercase letter. Do not end with a
period.

## Examples

```text
feat(client): add buffered command execution
```

```text
docs: add native SFTP design
```

```text
fix(sftp): reject malformed packet length
```
