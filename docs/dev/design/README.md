# Design Documents

Design documents are guide-level contracts for public API work.

Write them for users and implementors. Explain what people call, what they see,
what errors they handle, and how the behavior maps to `russh`. Avoid internal
module plans unless they affect public behavior.

Start from `_template.md`.

## Status

- Draft: open questions remain; do not implement public API from the document.
- Accepted: public API and behavior are stable enough to implement.
- Implementing: code is in progress and must stay aligned with the document.
- Implemented: code and tests match the document.

Before implementing non-trivial public API work, update the relevant design doc
to Accepted and make sure the roadmap points to it.

## Current Designs

- [Error Taxonomy](./error-taxonomy.md) - Implemented
- [Client Session API](./client-session-api.md) - Implemented
- [Loopback Test Fixtures](./loopback-test-fixtures.md) - Implemented
- [Server API](./server-api.md) - Implementing
- [Known Hosts](./known-hosts.md) - Implemented (first runtime slice)
- [Public Key and Agent Authentication](./public-key-auth.md) - Implemented (first runtime slice)
- [Channels and Shells](./channels-shells.md) - Implemented (first runtime slice)
- [Native SFTP Layer](./native-sftp.md) - Accepted
- [Forwarding and Tunnels](./forwarding-tunnels.md) - Implementing
