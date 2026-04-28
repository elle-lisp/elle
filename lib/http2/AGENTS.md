# lib/http2/

HTTP/2 implementation for Elle (RFC 9113 + RFC 7541 HPACK).

## Modules

| File | Purpose |
|------|---------|
| `huffman.lisp` | HPACK Huffman codec (RFC 7541 Appendix B) |
| `hpack.lisp` | HPACK header compression (static/dynamic tables, varint, string codec) |
| `frame.lisp` | HTTP/2 frame codec (9-byte header, 10 frame types, builders + CONTINUATION) |
| `stream.lisp` | Stream state machine + per-stream flow control |
| `session.lisp` | Shared session management (writer loop, send helpers, flow control) |
| `server.lisp` | Server connection handler + h2-serve |

The top-level module is `lib/http2.lisp` — see the lib/ AGENTS.md for
its full API documentation.

## Loading

```lisp
# Submodules (used internally by http2.lisp)
(def huffman ((import "std/http2/huffman")))
(def hpack   ((import "std/http2/hpack") :huffman huffman))
(def frame   ((import "std/http2/frame")))
(def stream  ((import "std/http2/stream") :sync sync :frame frame))
(def session ((import "std/http2/session") :sync sync :frame frame :stream stream :hpack hpack))
(def server  ((import "std/http2/server") :sync sync :hpack hpack :frame frame :stream stream :session session :tls tls))

# Top-level module (what users import)
(def http2 ((import "std/http2")))                    # h2c only
(def http2 ((import "std/http2") :tls tls))           # h2 over TLS
```

## Architecture

```
              TCP/TLS Connection
                    |
              +-----------+
              |  Reader   |  1 fiber per connection
              |  Fiber    |  reads frames, dispatches to stream queues
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
        |  Writer   |  1 fiber per connection
        |  Fiber    |  drains queue, batches writes
        +-----+-----+
```

- **Reader fiber**: reads frames, dispatches DATA/HEADERS to per-stream
  queues, handles SETTINGS/PING/GOAWAY/WINDOW_UPDATE on stream 0.
  Buffers CONTINUATION fragments until END_HEADERS before decoding.
- **Writer fiber**: drains bounded write queue, batches frame writes,
  flushes transport. Handles `:shutdown` sentinel for clean exit.
  On write error: sets closed?, notifies all streams.
- **Stream fibers**: one per request, block on data-queue:take
- **Handler fibers** (server): spawned per request, wrapped in
  protect+defer for error handling and stream cleanup

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
# Individual modules (use --home=. when running from a worktree)
echo '(let [m ((import "std/http2/huffman"))] (m:test))' | elle --home=.
echo '(let [m ((import "std/http2/frame"))] (m:test))' | elle --home=.

# Full module
echo '(let [m ((import "std/http2"))] (m:test))' | elle --home=.

# Integration tests
elle --home=. tests/elle/http2.lisp
elle --home=. tests/h2-server.lisp
```

## Invariants

1. Frame payloads are bytes, never strings. The tcp-transport buffers
   bytes (not strings) to avoid corrupting binary frame data.
2. HPACK dynamic tables are per-session, per-direction.
3. The writer fiber is the only writer to the transport after handshake.
   Handshake writes bypass the queue (writer not started yet).
4. `eprintln` is async in Elle and must NOT appear inside `let*` bindings
   — it yields to the scheduler between bindings, breaking atomicity.
5. Stream IDs: client uses odd (1, 3, 5...), server uses even (2, 4, 6...).
6. PUSH_PROMISE is rejected with RST_STREAM REFUSED_STREAM.
7. HPACK encode + send-frame must be non-yielding (atomic) — yielding
   between encode and send allows another fiber to encode with the same
   HPACK context, corrupting the dynamic table state.
8. Handler fibers are always wrapped in protect+defer: errors produce
   RST_STREAM or 500, streams are always cleaned up.
9. CONTINUATION frames: header blocks > max-frame-size are split across
   HEADERS + CONTINUATION frames. Receiver buffers fragments in
   pending-headers until END_HEADERS.
10. apply-remote-settings adjusts existing stream send windows by the
    delta (RFC 9113 Section 6.9.2).
