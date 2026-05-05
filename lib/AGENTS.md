# lib/

Reusable Elle modules. Each is a closure: `import-file` loads it, calling
the result initializes it and returns a struct of exports. Modules that
depend on other modules or plugins take them as arguments.

## Modules

| File | Purpose |
|------|---------|
| `http.lisp` | HTTP/1.1 client and server over TCP |
| `http2.lisp` | HTTP/2 client + module init + exports (h2 over TLS + h2c cleartext) |
| `http2/` | HTTP/2 submodules (huffman, hpack, frame, stream, session, server) — see [`http2/AGENTS.md`](http2/AGENTS.md) |
| `websocket.lisp` | WebSocket client and server (RFC 6455, ws:// and wss://) |
| `grpc.lisp` | gRPC client over HTTP/2 with length-prefixed framing |
| `tls.lisp` | TLS 1.2/1.3 client and server (with ALPN support) |
| `redis.lisp` | Redis client (RESP2) over TCP |
| `dns.lisp` | DNS client (RFC 1035) |
| `aws.lisp` | AWS client: SigV4 signing, HTTPS, service dispatch |
| `aws/` | AWS service modules (generated) + SigV4 signing — see [`aws/AGENTS.md`](aws/AGENTS.md) |
| `contract.lisp` | Compositional validation for function boundaries |
| `lua.lisp` | Lua standard library compatibility prelude |
| `process.lisp` | Erlang-style processes + GenServer, Actor, Supervisor |
| `irc.lisp` | IRCv3 client: CAP negotiation, SASL PLAIN, message tags, fiber read stream |
| `sync.lisp` | Concurrency primitives: lock, semaphore, condvar, rwlock, barrier, latch, once, queue, monitor (built on futex) |
| `spirv.lisp` | SPIR-V compute shader emitter: buffer I/O, arithmetic, loops, local variables, structured control flow |
| `gpu.lisp` | GPU compute convenience layer wrapping vulkan plugin + spirv emitter |

---

# lib/http

Agent guide for `lib/http.lisp` — Pure Elle HTTP/1.1 client and server.

## Purpose

HTTP/1.1 over TCP using Elle's existing stream and scheduler primitives.
Single file. No Rust changes (other than `port/path`, added in Chunk 0).

HTTPS and compression are opt-in via `&named` args on the module init:

```lisp
(def http ((import "std/http")))                     # http only

# HTTPS:
(def tls-plug ((import "plugin/tls")))
(def http ((import "std/http") :tls tls-plug))

# Compress helpers (gzip/zlib/deflate/zstd):
(def z ((import "std/compress")))
(def http ((import "std/http") :compress z))
#   — or have http import it for you:
(def http ((import "std/http") :compress true))

# Combined:
(def http ((import "std/http") :tls tls-plug :compress true))
```

Future plugins (DNS overrides, proxies, etc.) will be added as more
`&named` args on the same initializer.

## Data flow

Client:
```
http-get url → parse-url → tcp/connect → write-request-line → write-headers
→ port/flush → read-status-line → read-headers → read-body → port/close → response
```

Server:
```
http-serve port handler → tcp/listen → forever:
  tcp/accept → ev/spawn → defer(port/close):
    read-request → handler → write-response
```

## Exported functions

| Function | Signature | Effect | Returns |
|----------|-----------|--------|---------|
| `get` | `(fn [url &named headers query follow-redirects])` | Yields | response struct |
| `post` | `(fn [url body &named headers query follow-redirects])` | Yields | response struct |
| `request` | `(fn [method url &named body headers query follow-redirects])` | Yields | response struct |
| `serve` | `(fn [listener handler &named on-error])` | Yields | nil (runs forever) |
| `send` | `(fn [session method path &named body headers])` | Yields | response struct |
| `respond` | `(fn [status body &named headers])` | Silent | response struct |
| `parse-url` | `(fn [url])` | Errors | url struct |
| `query-encode` | `(fn [params])` | Silent | string |
| `header->kw` / `kw->header` | `(fn [name])` | Silent | keyword / string |
| `chunked?` | `(fn [headers])` | Silent | boolean |
| `write-chunk` / `write-last-chunk` | `(fn [t [data]])` | Yields | nil |
| `tcp-transport` / `tls-transport` | `(fn [port-or-conn])` | Silent | transport struct |
| `gzip` / `gunzip` / `zlib` / `unzlib` / `deflate` / `inflate` / `zstd` / `unzstd` | `(fn [data & opts])` | FFI | bytes |
| `sse-get` | `(fn [url &named headers last-event-id reconnect])` | Yields | fiber yielding events |
| `sse-response` | `(fn [body-fn &named headers])` | Silent | response struct |
| `format-sse-event` | `(fn [event])` | Silent | string |

### `:query` named arg

Accepts either a pre-encoded string or a struct. Struct values are
percent-encoded with `query-encode`:

- scalars (strings, integers, floats, keywords, booleans) → `key=value`
- arrays/lists → repeated `key=v1&key=v2`
- `nil` → dropped (useful for conditional parameters)

If the URL already carries a query (`?foo=bar`), `:query` is appended
with `&`. Keys iterate in struct order (sorted by key name).

### `:follow-redirects` named arg

Controls HTTP 3xx handling on the client:

| Value | Behavior |
|-------|----------|
| `nil` / `false` | Return the 3xx response as-is (default) |
| `true`           | Follow up to 10 hops |
| `<integer>`      | Follow up to N hops |

Per RFC 9110, `301` / `302` / `303` are followed with `GET` and an
empty body; `307` / `308` preserve the original method and body.
`Location` values may be absolute URLs, scheme-relative (`//host/path`),
or absolute paths (`/path`). On hop exhaustion the last redirect
response is returned to the caller.

### `:compress` — raw helpers

No auto-negotiation: supplying `:compress` merely exposes
gzip/gunzip/zlib/unzlib/deflate/inflate/zstd/unzstd as `http:<name>`.
Callers apply them explicitly to bodies or individual chunks. Calls
signal `:http-error :compress-not-configured` if `:compress` was not
passed at module init.

## Server-Sent Events

| Function | Signature | Returns |
|----------|-----------|---------|
| `sse-get` | `(fn [url &named headers last-event-id reconnect])` | fiber |
| `sse-response` | `(fn [body-fn &named headers])` | response struct |
| `format-sse-event` | `(fn [event])` | string (SSE wire frame) |

**Client**: `sse-get` returns a fiber (with `|:yield|` mask) yielding event structs
`{:event :data :id :retry}`. Iterate with `each`:

```lisp
(each evt in (http:sse-get "http://server/stream")
  (println evt:event ": " evt:data))
```

`:reconnect` defaults to `true` — the client follows EventSource
semantics: on disconnect/error it waits `retry-ms` (last server-sent
`:retry`, default 3000) and reopens with `Last-Event-ID`. Stops on HTTP
204 No Content. Set `:reconnect false` to make the fiber end on
the first disconnect.

**Server**: return an `http:sse-response` from a handler. The body
closure receives a `send-event` function:

```lisp
(defn handler [req]
  (http:sse-response
    (fn [send-event]
      (send-event {:data "first"})
      (send-event {:event "tick" :data "1" :id "a"})
      (send-event {:retry 5000}))))
```

Under the hood `sse-response` sets `Content-Type: text/event-stream`
plus `Transfer-Encoding: chunked` and reuses the existing chunked-body
streaming path; each `send-event` call becomes one chunk on the wire.

## Struct shapes

**Request** (produced by server, consumed by handler):
```lisp
{:method "GET" :path "/foo" :version "HTTP/1.1"
 :headers {:host "example.com" :content-type "text/plain"}
 :body "..." # or nil
}
```

**Response** (produced by handler or http-respond, consumed by serializer):
```lisp
{:status 200
 :headers {:content-type "text/plain" :content-length "5"}
 :body "hello" # or nil
}
```

**URL** (produced by parse-url):
```lisp
{:scheme "http"  :host "example.com" :port 80  :path "/foo" :query "page=1"}
{:scheme "https" :host "example.com" :port 443 :path "/foo" :query nil}
```

## parse-url function

### Signature

```lisp
(parse-url url) → {:scheme :host :port :path :query}
```

### Parameters

- `url` (string): URL to parse. Must start with `"http://"` or `"https://"`.

### Returns

Immutable struct with keys:
- `:scheme` (string): `"http"` or `"https"`
- `:host` (string): Hostname or IP address
- `:port` (integer): Port number, defaults to 80 (http) or 443 (https) if absent
- `:path` (string): Request path, default `"/"` if absent
- `:query` (string or nil): Query string after `?`, or nil if absent

### Error cases

Signals `:http-error` (via `error` form) for:
1. **Unsupported scheme**: URL does not start with `"http://"` or `"https://"` (e.g., `"ftp://"`, `"wss://"`)
2. **Missing host**: URL is `"http://"` with nothing after
3. **Empty host**: Authority part is empty (e.g., `"http://:8080/"`)
4. **Malformed port**: Port part is not a valid integer (e.g., `"http://example.com:abc/"`)

Error value is a struct: `{:error :http-error :reason ... :url ... :message "..."}`

### Examples

```lisp
(parse-url "http://example.com:8080/api/users?page=2")
# → {:scheme "http" :host "example.com" :port 8080 :path "/api/users" :query "page=2"}

(parse-url "http://example.com/index.html")
# → {:scheme "http" :host "example.com" :port 80 :path "/index.html" :query nil}

(parse-url "https://example.com")
# → {:scheme "https" :host "example.com" :port 443 :path "/" :query nil}

(parse-url "https://api.example.com:8443/v1/items?limit=10")
# → {:scheme "https" :host "api.example.com" :port 8443 :path "/v1/items" :query "limit=10"}

(parse-url "ftp://example.com/")
# → error {:error :http-error :reason :unsupported-scheme ...}
```

Note: `parse-url` supports the `https` scheme unconditionally. `http:get`
/ `http:post` / `http:request` only speak HTTPS when a TLS plugin was
supplied via `((import "std/http") :tls tls-plug)` at module init;
otherwise an `https://` URL signals `:http-error :tls-not-configured`.

## Transport abstraction

All wire-format helpers operate on a **transport**: a struct of closures
`{:read :read-line :write :flush :close}`. Transports are produced by
`tcp-transport` (plain TCP port) or `tls-transport` (TLS connection from
the configured TLS plugin). `open-transport` picks the right one based
on the parsed URL's scheme. This means the chunked reader, body reader,
headers code, etc. are shared across http and https with no duplication.

Session structs returned by `http:connect` now carry `:transport` rather
than `:conn`; the underlying port or TLS conn is reachable via
`session:transport`'s closures.

## Invariants

1. Header keys are always lowercase keywords after parsing.
2. Content-Length is always set in responses produced by `http:respond`.
3. Connections are always closed via `defer` — never leaked on error.
4. `http:serve` absorbs handler errors (500 response); server keeps running.
5. `http://` always uses plain TCP. `https://` requires `:tls` to have
   been passed to the module initializer; otherwise client calls
   signal `:http-error :tls-not-configured`.
6. `http:serve` serves plain HTTP only. HTTPS serving is not first-class
   yet; users can wrap accepted TCP connections with `tls:accept` and
   feed the result into `connection-loop` via `tls-transport`.
7. Chunked transfer encoding is supported on both read and write.
   `Transfer-Encoding: chunked` takes precedence over `Content-Length`.
8. No connection pooling. Each `http:get`/`http:post` opens and closes
   a transport.

## Running tests

```bash
./target/debug/elle tests/elle/http.lisp
```

---

# lib/http2

Agent guide for `lib/http2.lisp` — HTTP/2 client and server (RFC 9113 + RFC 7541).

## Purpose

HTTP/2 over TCP (h2c cleartext) and TLS (h2 with ALPN). Submodules in
`lib/http2/` handle Huffman coding, HPACK header compression, frame codec,
stream state management, shared session infrastructure, and server logic.

## Loading

```lisp
# h2c only (cleartext HTTP/2)
(def http2 ((import "std/http2")))

# h2 over TLS (requires TLS plugin)
(def tls-plug ((import "plugin/tls")))
(def tls ((import "std/tls") tls-plug))
(def http2 ((import "std/http2") :tls tls))
```

## Exported functions

| Function | Signature | Returns |
|----------|-----------|---------|
| `get` | `(fn [url &named headers])` | response struct |
| `post` | `(fn [url body &named headers])` | response struct |
| `request` | `(fn [method url &named body headers])` | response struct |
| `connect` | `(fn [url])` | session struct |
| `send` | `(fn [session method path &named body headers])` | response struct |
| `close` | `(fn [session])` | nil |
| `serve` | `(fn [listener handler &named tls-config on-error])` | nil (runs forever) |
| `parse-url` | `(fn [url])` | url struct |

## Struct shapes

**Response**: `{:status 200 :headers {:content-type "text/html"} :body <bytes>}`

**Session**: `@{:transport :is-server? :streams :next-stream-id :hpack-encoder :hpack-decoder :local-settings :remote-settings :conn-flow :write-queue :reader-fiber :writer-fiber :closed? :host}`

## Architecture

Fiber-per-stream multiplexing: one reader fiber dispatches frames to
per-stream queues, one writer fiber drains a bounded write queue. Stream
fibers block on their data queue waiting for response frames.

## Invariants

1. Frame payloads are bytes. The tcp-transport buffers bytes, not strings.
2. HPACK dynamic tables are per-session, per-direction.
3. The writer fiber is the only writer after handshake. Handshake writes
   bypass the queue (writer not started yet).
4. PUSH_PROMISE is rejected with RST_STREAM REFUSED_STREAM.
5. Stream IDs: client odd (1, 3, 5...), server even (2, 4, 6...).

## Running tests

```bash
elle --home=. tests/elle/http2.lisp
elle --home=. tests/h2-server.lisp
elle --home=. tests/h2-same-scheduler.lisp
elle --home=. tests/h2-flow-control.lisp
```

---

# lib/tls

Agent guide for `lib/tls.lisp` — TLS client and server using the `elle-tls` plugin.

## Purpose

TLS 1.2 and 1.3 over TCP using Elle's existing stream and scheduler primitives.
The plugin provides the state machine; Elle code handles all I/O via native TCP ports.

## Data flow

Client:
```
tls:connect host port → tcp/connect → tls/client-state → tls-handshake loop → tls-conn

tls-handshake:
  loop: tls/process(bytes) → send outgoing → check handshake-complete? → read more
```

Server:
```
tls:accept listener config → tcp/accept → tls/server-state → tls-handshake loop → tls-conn
```

Read:
```
tls:read conn n → check plaintext buffer → port/read TCP → tls/process → tls/read-plaintext
```

Write:
```
tls:write conn data → tls/encrypt → port/write TCP
```

Stream:
```
tls:lines conn → fiber/new |:yield| (loop: tls:read-line → yield)
```

## Exported functions

| Function | Signature | Returns | Notes |
|----------|-----------|---------|-------|
| `tls:connect` | `host port [opts]` | tls-conn | async |
| `tls:accept` | `listener config` | tls-conn | async |
| `tls:server-config` | `cert-path key-path` | tls-server-config | sync |
| `tls:read` | `conn n` | bytes or nil | async |
| `tls:read-line` | `conn` | string or nil | async |
| `tls:read-all` | `conn` | bytes | async |
| `tls:write` | `conn data` | int | async |
| `tls:close` | `conn` | nil | async |
| `tls:lines` | `conn` | fiber | stream |
| `tls:chunks` | `conn size` | fiber | stream |
| `tls:writer` | `conn` | fiber | stream |

## tls-conn shape

```lisp
{:tcp port      # TcpStream port — raw TCP connection
 :tls tls-state # TlsState ExternalObject from elle-tls plugin}
```

## Invariants

1. All functions yield (async I/O).
2. `tls:close` always closes the TCP port, even if `close_notify` fails.
3. After every `tls/process` call, outgoing data must be drained and sent.
4. `tls:lines` and `tls:chunks` close the connection when exhausted.
5. The `tls-conn` struct is transparent and inspectable; `:tcp` and `:tls` fields
   are accessible directly.

## Running tests

```bash
./target/debug/elle tests/elle/tls.lisp
```

---

# lib/redis

Agent guide for `lib/redis.lisp` — Pure Elle Redis client (RESP2).

## Purpose

Redis client over TCP using Elle's async I/O primitives. Single file. No Rust
plugin. Speaks RESP2.

## Data flow

```
redis:with host port thunk → tcp/connect → parameterize(*redis-port*) → thunk
  redis:set "key" "val" → redis-cmd → resp-encode → port/write → port/flush → resp-read → resp-ok?
  redis:get "key"        → redis-cmd → resp-encode → port/write → port/flush → resp-read
```

Manager:
```
redis:manager host port → {:run fn :port-param param}
  run thunk → connect → defer(close) → loop: parameterize → thunk
    on error: terminal? → crash | reconnect → retry
```

## Exported functions

| Function | Signature | Returns | Notes |
|----------|-----------|---------|-------|
| `redis:connect` | `host port` | TCP port | async |
| `redis:close` | `port` | nil | |
| `redis:with` | `host port thunk` | thunk result | async, manages lifecycle |
| `redis:manager` | `host port [&named terminal? max-retries]` | manager struct | |
| `redis:get` | `key` | string or nil | uses `*redis-port*` |
| `redis:set` | `key value [&named ex px nx xx]` | true or string | uses `*redis-port*` |
| `redis:hgetall` | `key` | struct (string keys) | uses `*redis-port*` |
| `redis:subscribe` | `port & channels` | port | enters sub mode |
| `redis:recv` | `port` | `{"channel" ... "data" ...}` or nil | |
| `redis:pipeline` | `& commands` | array of results | uses `*redis-port*` |
| `redis:ping` | | "PONG" | uses `*redis-port*` |
| `redis:test` | | true | RESP self-tests |

Full command list: strings, keys, hashes, lists, sets, sorted sets, server,
pub/sub. See export struct at bottom of file.

## Connection model

The connection is a bare TCP port. No wrapper struct. `*redis-port*` is a
`Parameter` that holds the current connection for ambient access by commands.

Two usage patterns:
1. **`redis:with`** — simple: opens connection, binds parameter, runs thunk,
   closes on exit.
2. **`redis:manager`** — resilient: reconnects on non-terminal errors, crashes
   on terminal ones.

## Value mapping

| Redis | Elle |
|-------|------|
| Nil bulk string (`$-1`) | `nil` |
| Integer reply | integer |
| Simple string | string |
| Array reply | array (immutable) |
| HGETALL | struct with string keys |
| EXISTS/HEXISTS/SISMEMBER/EXPIRE | boolean |
| OK replies | `true` |

## Invariants

1. All commands require `*redis-port*` to be bound (via `redis:with` or
   `redis:manager`).
2. Pub/sub functions take the raw port directly (not through the parameter).
3. `resp-read-raw` returns error structs; `resp-read` signals errors.
4. HGETALL returns string keys, not keyword keys.
5. `string/size-of` is used for bulk string length (byte length, not grapheme
   count).

## Running tests

```bash
# RESP self-tests only (no Redis needed)
echo '((import-file "lib/redis.lisp"):test)' | ./target/debug/elle

# Full integration tests (requires Redis on 127.0.0.1:6379)
./target/debug/elle tests/elle/redis.lisp
```

---

# lib/process

Agent guide for `lib/process.lisp` — Erlang-style processes with GenServer,
Actor, and Supervisor.

## Purpose

Cooperative multitasking with message passing, links, monitors, timers, and
named process registration. Built on fibers with fuel-based preemption.
GenServer/Actor/Supervisor are OTP-like abstractions layered on top.

## Exported functions

### Process primitives (yield-based, used inside processes)

| Function | Signature | Returns |
|----------|-----------|---------|
| `send` | `pid msg` | `:ok` |
| `recv` | | message |
| `recv-match` | `pred` | matching message |
| `recv-timeout` | `ticks` | message or `:timeout` |
| `self` | | pid |
| `spawn` | `closure` | pid |
| `spawn-link` | `closure` | pid |
| `spawn-monitor` | `closure` | `[pid ref]` |
| `link` / `unlink` | `pid` | `:ok` |
| `monitor` / `demonitor` | `pid` / `ref` | ref / `:ok` |
| `trap-exit` | `bool` | `:ok` |
| `exit` | `pid reason` | `:ok` |
| `register` / `unregister` | `name` | `:ok` |
| `whereis` | `name` | pid or nil |
| `send-named` | `name msg` | `:ok` |
| `send-after` | `ticks pid msg` | timer-ref |
| `cancel-timer` | `ref` | `:ok` or `:not-found` |
| `put-dict` / `get-dict` / `erase-dict` | `key [val]` | old / val / old |

### GenServer

| Function | Signature | Returns |
|----------|-----------|---------|
| `gen-server-start-link` | `callbacks init-arg &named name` | pid |
| `gen-server-call` | `server request &named timeout` | reply |
| `gen-server-cast` | `server request` | `:ok` |
| `gen-server-stop` | `server &named reason timeout` | `:ok` |
| `gen-server-reply` | `from reply` | `:ok` |

`server` is a pid or registered name (keyword). `from` is `[pid ref]`.

Callbacks struct:
```lisp
{:init        (fn [arg] state)
 :handle-call (fn [request from state] [:reply reply state] | [:noreply state] | [:stop reason reply state])
 :handle-cast (fn [request state]      [:noreply state] | [:stop reason state])
 :handle-info (fn [msg state]          [:noreply state] | [:stop reason state])
 :terminate   (fn [reason state] ...)}
```

### Actor

| Function | Signature | Returns |
|----------|-----------|---------|
| `actor-start-link` | `init-fn &named name` | pid |
| `actor-get` | `actor fun` | `(fun state)` |
| `actor-update` | `actor fun` | `:ok` |
| `actor-cast` | `actor fun` | `:ok` |

### Task

| Function | Signature | Returns |
|----------|-----------|---------|
| `task-async` | `fun` | `[pid ref]` |
| `task-await` | `task &named timeout` | result |

### Supervisor

| Function | Signature | Returns |
|----------|-----------|---------|
| `supervisor-start-link` | `children &named name strategy` | pid |
| `supervisor-start-child` | `sup spec` | child pid |
| `supervisor-stop-child` | `sup id` | `:ok` |
| `supervisor-which-children` | `sup` | `[{:id :pid} ...]` |

Child spec: `{:id keyword :start (fn [] ...) :restart :permanent|:transient|:temporary}`

Strategies: `:one-for-one` (default), `:one-for-all`, `:rest-for-one`

### EventManager

| Function | Signature | Returns |
|----------|-----------|---------|
| `event-manager-start-link` | `&named name` | pid |
| `event-manager-add-handler` | `manager mod init-arg` | handler ref |
| `event-manager-remove-handler` | `manager ref` | `:ok` |
| `event-manager-notify` | `manager event` | `:ok` (async) |
| `event-manager-sync-notify` | `manager event` | `:ok` (sync) |
| `event-manager-which-handlers` | `manager` | `[{:id :mod} ...]` |

Handler module: `{:init (fn [arg] state) :handle-event (fn [event state] [:ok state]|[:remove state]) :terminate (fn [reason state] ...)}`

### External API (outside processes)

| Function | Signature | Returns |
|----------|-----------|---------|
| `make-scheduler` | `&named fuel backend` | scheduler struct |
| `start` | `init &named fuel backend` | scheduler |
| `run` | `sched init` | nil |
| `process-info` | `sched pid` | info struct |
| `inject` | `sched pid msg` | nil |

## Running tests

```bash
./target/debug/elle tests/elle/process.lisp
./target/debug/elle tests/elle/genserver.lisp
```

---

# lib/irc

Agent guide for `lib/irc.lisp` -- IRCv3 client.

## Purpose

IRC client with IRCv3 capability negotiation, SASL PLAIN authentication,
and message tags. Fiber-based: the connection struct exposes a read
stream (a fiber with `|:yield|` mask) and a send function. No background
fibers -- the caller controls when messages are consumed.

## Loading

```lisp
# Plain TCP
(def irc ((import "std/irc")))

# TLS (standard for modern IRC)
(def tls ((import "std/tls") (import "plugin/tls")))
(def irc ((import "std/irc") :tls tls))
```

## Data flow

```
irc:connect host port :nick :sasl [user pass]
  -> TCP or TLS connect
  -> CAP LS 302 + NICK + USER
  -> CAP negotiation (LS/REQ/ACK)
  -> SASL PLAIN if negotiated
  -> CAP END
  -> wait for 001 RPL_WELCOME
  -> return conn struct
```

## Connection struct

```lisp
{:messages <fiber>       # yields parsed messages, auto-PONGs (fiber/new f |:yield|)
 :send     <function>    # (conn:send "COMMAND" "param1" ...)
 :close    <function>    # sends QUIT, closes transport
 :nick     "nick"        # resolved nick after registration
 :caps     |"multi-prefix" "server-time"|  # negotiated capabilities
 :server   "irc.libera.chat"
 :isupport {:chantypes "#&" :prefix "(ov)@+"}}
```

## Message struct

```lisp
{:tags    {:time "2024-01-01T00:00:00Z" :msgid "abc123"}
 :source  {:nick "user" :user "ident" :host "host.com"}
 :command "PRIVMSG"
 :params  ["#channel" "Hello world"]}
```

## Exported functions

| Function | Signature | Returns |
|----------|-----------|---------|
| `connect` | `(fn [host port &named nick username realname sasl])` | conn struct |
| `parse-message` | `(fn [line])` | message struct |
| `format-message` | `(fn [msg])` | string |
| `parse-tags` | `(fn [raw])` | struct |
| `parse-source` | `(fn [raw])` | struct |
| `parse-ctcp` | `(fn [text])` | `{:command :text}` or nil |
| `test` | `(fn [])` | true |

## Invariants

- `conn:messages` only yields non-PING messages; PINGs are auto-answered
- `conn:send` formats the trailing parameter automatically (`:` prefix when needed)
- Registration handles nick collision by appending `_` (up to 3 retries)
- All I/O yields to the scheduler (async)

## Concurrency patterns

```lisp
# Simple bot (single fiber)
(each msg in conn:messages
  (when (= msg:command "PRIVMSG")
    (conn:send "PRIVMSG" (get msg:params 0) "reply")))

# Background reader (multi-fiber)
(def sync ((import "std/sync")))
(def q (sync:make-queue 256))
(ev/spawn (fn [] (each msg in conn:messages (q:put msg))))
```
