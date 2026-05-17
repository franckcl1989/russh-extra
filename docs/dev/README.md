# russh-extra Developer Documentation

Documentation for contributors and AI agents working on `russh-extra`.

Start with `CONTRIBUTING.md`, then read the project charter, constraints,
architecture, and roadmap docs.

## Project Direction

- [Project Charter](./project-charter.md)
- [Development Constraints](./constraints.md)
- [AI Development Workflow](./ai-workflow.md)
- [Testing Strategy](./testing.md)
- [Development Plan](./development-plan.md)
- [0.1.1 Development Plan](./0.1.1-development-plan.md)
- [0.1.2 Development Plan](./0.1.2-development-plan.md)
- [0.1.3 Development Plan](./0.1.3-development-plan.md)
- [Security Policy](./security.md)
- [Release and Compatibility Policy](./release.md)

## Agent Workflow

For non-trivial public API work:

1. Update `docs/dev/roadmap.md`.
2. Write or update a design doc under `docs/dev/design/`.
3. Keep the design in Draft while blocking questions remain.
4. Mark the design Accepted only after public API, behavior, errors, security,
   feature flags, and tests are specified.
5. Implement against the Accepted design and update it if `russh` API limits are
   discovered.

## Architecture

- [Architecture Overview](./architecture/README.md)

## Design Documents

Guide-level design documents for public API work live in `docs/dev/design/`.
Use [`_template.md`](./design/_template.md) when starting a new one.

- [Design Overview](./design/README.md)
- [Error Taxonomy](./design/error-taxonomy.md)
- [Client Session API](./design/client-session-api.md)
- [Loopback Test Fixtures](./design/loopback-test-fixtures.md)
- [Server API](./design/server-api.md)
- [Known Hosts](./design/known-hosts.md)
- [Public Key and Agent Authentication](./design/public-key-auth.md)
- [Channels and Shells](./design/channels-shells.md)
- [Native SFTP Layer](./design/native-sftp.md)
- [Forwarding and Tunnels](./design/forwarding-tunnels.md)

## Roadmap

- [Roadmap](./roadmap.md)

## Decisions and Audits

- [Architecture Decision Records](./decisions/README.md)
- [Independent AI Audit Records](./audits/README.md)

## Project

- [Commit Guidelines](./COMMITS.md)
- [GitHub Labels](./labels.md)
