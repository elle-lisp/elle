# lib/http2/

HTTP/2 implementation for Elle (RFC 9113 + RFC 7541 HPACK).

## Modules

| File | Purpose |
|------|---------|
| `huffman.lisp` | HPACK Huffman codec (RFC 7541 Appendix B) |
| `hpack.lisp` | HPACK header compression (static/dynamic tables, varint, string codec) |
| `frame.lisp` | HTTP/2 frame codec (9-byte header, 10 frame types, builders + CONTINUATION) |
| `stream.lisp` | Stream state machine + per-stream flow control |
| `transport.lisp` | Transport abstraction (tcp-transport, tls-transport) |
| `session.lisp` | Session management, shared reader loop, writer loop, send helpers, flow control |
| `server.lisp` | Server connection handler + h2-serve |

The top-level module is `lib/http2.lisp` — see the lib/ AGENTS.md for
its full API documentation.

## Loading

```lisp
# Submodules (used internally by http2.lisp)
(def huffman   ((import "std/http2/huffman")))
(def hpack     ((import "std/http2/hpack") :huffman huffman))
(def frame     ((import "std/http2/frame")))
(def stream    ((import "std/http2/stream") :sync sync :frame frame))
(def transport ((import "std/http2/transport") :tls tls))
(def session   ((import "std/http2/session") :sync sync :frame frame :stream stream :hpack hpack))
(def server    ((import "std/http2/server") :sync sync :hpack hpack :frame frame :stream stream :session session :tls tls :transport transport))

# Top-level module (what users import)
(def http2 ((import "std/http2")))                    # h2c only
(def http2 ((import "std/http2") :tls tls))           # h2 over TLS
```

## Architecture

```
              TCP/TLS Connection
                    |
              +-----------+
              |  Reader   |  session:read-loop (shared)
              |  Loop     |  on-headers / on-goaway callbacks
              +-----+-----+
                    |
        +-----------+-----------+
        |           |           |
  +-----+-----+ +---+---+ +---+---+
  | Stream 1  | | Str 3 | | Str 5 |  sync:make-queue per stream
  | data queue| | queue | | queue |  blocks on take()
  +-----+-----+ +---+---+ +---+---+
        |           |           |
        +-----+-----+-----------+
              |
        +-----+-----+
        |Write Queue|  bounded sync:make-queue
        +-----+-----+
              |
        +-----+-----+
        |  Writer   |  session:writer-loop
        |  Loop     |  drains queue, batches writes
        +-----+-----+
```

### Key design decisions

- **Shared reader loop**: `session:read-loop` handles all frame types
  identically for client and server. Two callbacks differ:
  - `on-headers`: client enqueues headers to stream queue; server spawns
    handler fiber
  - `on-goaway`: client tracks goaway-recvd?; server shuts down (returns
    truthy to break reader loop)

- **Single transport definition**: `transport.lisp` defines tcp-transport
  and tls-transport once. Both http2.lisp and server.lisp import it.

- **Atomic HPACK encode+send**: `encode-and-send-headers` encodes and
  enqueues all frames (HEADERS + CONTINUATION) without yielding, preventing
  HPACK dynamic table corruption.

## Data shapes

**Frame** (from `frame:read-frame`):
```lisp
@{:length 42 :type 1 :flags 0x5 :stream-id 1 :payload <bytes>}
```

**Stream** (from `stream:make-stream`):
```lisp
@{:id 1 :state :open :flow <flow-control> :recv-window 65535
  :data-queue <queue> :headers nil :error-code nil}
```

**HPACK header list**: `[["name" "value"] ...]`

## Testing

```bash
# Individual modules
elle --home=. tests/http2/modules.lisp

# Integration tests
elle --home=. tests/http2/all.lisp
```

## Invariants

1. Frame payloads are bytes, never strings.
2. HPACK dynamic tables are per-session, per-direction.
3. The writer fiber is the only writer to the transport after handshake.
4. `eprintln` is async in Elle — do not use inside `let*` bindings.
5. Stream IDs: client odd (1, 3, 5...), server even (2, 4, 6...).
6. PUSH_PROMISE is rejected with RST_STREAM REFUSED_STREAM.
7. HPACK encode + send-frame must be non-yielding (atomic).
8. Handler fibers are always wrapped in protect+defer.
9. CONTINUATION: header blocks > max-frame-size split across frames.
10. apply-remote-settings adjusts existing stream send windows by delta.
11. SETTINGS values are validated (ENABLE_PUSH 0/1, INITIAL_WINDOW_SIZE
    <= 2^31-1, MAX_FRAME_SIZE 16384..16777215).
12. Zero WINDOW_UPDATE increment is rejected per RFC 9113.
13. PADDED flag handling strips padding from DATA/HEADERS payloads.
14. local-settings and remote-settings are mutable structs (@{}).
