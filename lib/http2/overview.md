# http2

<!-- audited: 2026-09-14 -->

The submodules behind [http2.lisp](../http2.lisp): HPACK, the frame codec, stream state, the session loops and the server.

Each file's header says how to load it and what it exports, and
`(doc name)` carries a function's arguments. This file holds the parts
no single submodule owns: how the fibers divide the connection, the
decisions that shape them, and the invariants that cross files.

| File | Purpose |
|------|---------|
| `huffman.lisp` | HPACK Huffman codec (RFC 7541 Appendix B) |
| `hpack.lisp` | Header compression: static and dynamic tables, varint, string codec |
| `frame.lisp` | Frame codec: 9-byte header, ten frame types, builders, CONTINUATION |
| `stream.lisp` | Stream state machine, per-stream flow control, the channel |
| `transport.lisp` | Transport abstraction over TCP and TLS |
| `session.lisp` | Session state, the writer loop and its shutdown, the send side |
| `reader.lisp` | The frame reader both roles run |
| `server.lisp` | Server connection handler and the accept loop |

## How a connection divides

One reader fiber owns the socket's read side. It decodes every frame and
hands the result to a channel per stream, so a stream's consumer blocks
on its own queue and never on the socket. One writer fiber owns the
write side, draining the session's unbounded write queue and batching
what it finds there into the transport. Nothing else writes after the
handshake.

## Design decisions

- **One reader loop for both roles.** `reader:read-loop` handles every
  frame type the same way for a client and a server. Two callbacks
  differ: `on-headers` enqueues on the client and spawns a handler on
  the server, and `on-goaway` records the state on the client and stops
  the loop on the server. The reader imports the session and never the
  other way round, so the send side knows nothing about who reads.

- **One transport definition.** `transport.lisp` builds the TCP and TLS
  transports once, and both the client and the server import it.

- **HPACK encode and send are atomic.** `encode-and-send-headers`
  encodes and enqueues HEADERS plus every CONTINUATION without yielding.
  A yield between them lets another fiber encode against the same
  dynamic table, which corrupts it for the peer as well.

- **A close cannot wait on the peer.** A peer that stops reading parks
  the writer fiber inside `port/write`, and that write carries no
  deadline. So the close races the writer against a timer and aborts the
  writer when the timer wins. Joining the writer outright hands the peer
  control over when the close returns, which is the wedge
  `tests/elle/h2-close-on-dead-peer.lisp` holds shut.

## Invariants

1. Frame payloads are bytes, never strings.
2. HPACK dynamic tables are per session and per direction.
3. After the handshake the writer fiber is the only writer. Handshake
   writes go straight to the transport, before that fiber starts.
4. Stream ids: client odd, server even.
5. PUSH_PROMISE draws RST_STREAM REFUSED_STREAM.
6. A handler fiber always runs inside `protect` and `defer`.
7. A header block over max-frame-size splits across CONTINUATION frames.
8. `apply-remote-settings` shifts every existing stream's send window by
   the delta.
9. SETTINGS values are validated: ENABLE_PUSH is 0 or 1,
   INITIAL_WINDOW_SIZE is at most 2^31-1, MAX_FRAME_SIZE falls in
   16384..16777215.
10. A WINDOW_UPDATE increment of zero is refused, per RFC 9113.
11. The PADDED flag's padding is stripped from DATA and HEADERS payloads.
12. `local-settings` and `remote-settings` are mutable structs.
13. Closing a session returns in bounded time, whatever the peer does.
    The server's connection handler waits under the same bound.

## Running tests

```bash
elle tests/http2/modules.lisp
elle tests/http2/all.lisp
```
