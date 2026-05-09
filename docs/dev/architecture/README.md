# russh-extra Architecture Overview

`russh-extra` is a high-level async SSH API for Rust built on top of the
official `russh` crate. It covers the main client, server, shell, SFTP, and
forwarding workflows, while leaving lower-level `russh` controls reachable
through explicit escape hatches.

The project does not wrap community SSH or SFTP helper crates. Higher-level
client sessions, server handlers, SFTP, shells, and tunnels are implemented in
this repository using `russh` connections, handlers, channels, and subsystem
requests.

## Project Structure

The repository is a Cargo workspace with separate crates for public API,
shared contracts, and tests.

| Crate | Purpose |
|---|---|
| `russh-extra` | User-facing API for clients, servers, channels, SFTP, shells, and tunnels |
| `russh-extra-core` | Shared types: config, auth, endpoints, channel metadata, forwarding specs, errors |
| `russh-extra-test-support` | Integration test helpers and local SSH fixtures |
| `russh-extra-tests` | Workspace-level API and integration tests |

## Layers

### Core Types

`russh-extra-core` owns stable, feature-neutral domain types. These types avoid
depending on runtime handles so they can be reused by the main crate, macros,
tests, examples, and future tooling.

### Runtime API

`russh-extra` owns runtime behavior. It should expose concise builders and typed
handles while preserving SSH concepts:

- `Client` creates authenticated sessions.
- `Session` opens command, shell, subsystem, SFTP, and forwarding channels.
- `Server` accepts connections and dispatches authentication and channel
  requests to user handlers.
- `Shell` and `Tunnel` are typed high-level handles over `russh` channels.
- `SftpClient` and `SftpServerHandler` implement SFTP v3 over the SSH `sftp`
  subsystem.

## Protocol Ownership

SFTP and forwarding are first-class `russh-extra` features. Their behavior must
be specified in design docs before implementation:

- SFTP runs over the SSH `sftp` subsystem and implements protocol framing in
  this repository.
- Local and remote TCP forwarding use `direct-tcpip`, `forwarded-tcpip`, and
  global forwarding requests exposed by `russh`.
- PTY and shell APIs use normal session channel requests.

## Escape Hatches

High-level types should provide access to lower-level `russh` handles where it
is useful and safe. Users who need an unsupported SSH feature should be able to
drop down without abandoning the connection.

## Testing Strategy

Most public behavior should be tested through integration tests. Unit tests are
appropriate for small parsers, validators, SFTP packet encoding, and forwarding
spec conversion.

Network tests should use local loopback servers from `russh-extra-test-support`
so CI does not depend on external SSH hosts.
