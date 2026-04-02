# lib/

Reusable Elle modules. Each is a closure: `import-file` loads it, calling
the result initializes it and returns a struct of exports. Modules that
depend on other modules or plugins take them as arguments.

## Modules

| File | Purpose |
|------|---------|
| `http.lisp` | HTTP/1.1 client and server over TCP |
| `tls.lisp` | TLS 1.2/1.3 client and server |
| `redis.lisp` | Redis client (RESP2) over TCP |
| `dns.lisp` | DNS client (RFC 1035) |
| `aws.lisp` | AWS client: SigV4 signing, HTTPS, service dispatch |
| `aws/` | AWS service modules (generated) + SigV4 signing — see [`aws/AGENTS.md`](aws/AGENTS.md) |
| `contract.lisp` | Compositional validation for function boundaries |
| `lua.lisp` | Lua standard library compatibility prelude |
| `process.lisp` | Erlang-style processes + GenServer, Actor, Supervisor |
| `sync.lisp` | Concurrency primitives: lock, semaphore, condvar, rwlock, barrier, latch, once, queue, monitor (built on futex) |

---

# lib/http

Agent guide for `lib/http.lisp` — Pure Elle HTTP/1.1 client and server.

## Purpose

HTTP/1.1 over TCP using Elle's existing stream and scheduler primitives.
Single file. No Rust changes (other than `port/path`, added in Chunk 0).

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
| `http-get` | `(fn [url &named headers])` | Yields | response struct |
| `http-post` | `(fn [url body &named headers])` | Yields | response struct |
| `http-request` | `(fn [method url &named body headers])` | Yields | response struct |
| `http-serve` | `(fn [listener handler &named on-error])` | Yields | nil (runs forever) |
| `http-send` | `(fn [session method path &named body headers])` | Yields | response struct |
| `http-respond` | `(fn [status body &named headers])` | Silent | response struct |
| `parse-url` | `(fn [url])` | Errors | url struct |

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
{:scheme "http" :host "example.com" :port 80 :path "/foo" :query "page=1"}
```

## parse-url function

### Signature

```lisp
(parse-url url) → {:scheme :host :port :path :query}
```

### Parameters

- `url` (string): HTTP URL to parse. Must start with `"http://"`.

### Returns

Immutable struct with keys:
- `:scheme` (string): Always `"http"` (only scheme supported)
- `:host` (string): Hostname or IP address
- `:port` (integer): Port number, default 80 if absent
- `:path` (string): Request path, default `"/"` if absent
- `:query` (string or nil): Query string after `?`, or nil if absent

### Error cases

Signals `:http-error` (via `error` form) for:
1. **Unsupported scheme**: URL does not start with `"http://"` (e.g., `"ftp://"`, `"https://"`)
2. **Missing host**: URL is `"http://"` with nothing after
3. **Empty host**: Authority part is empty (e.g., `"http://:8080/"`)
4. **Malformed port**: Port part is not a valid integer (e.g., `"http://example.com:abc/"`)

Error value is a struct: `{:error :http-error :message "..."}`

### Examples

```lisp
(parse-url "http://example.com:8080/api/users?page=2")
# → {:scheme "http" :host "example.com" :port 8080 :path "/api/users" :query "page=2"}

(parse-url "http://example.com/index.html")
# → {:scheme "http" :host "example.com" :port 80 :path "/index.html" :query nil}

(parse-url "http://example.com")
# → {:scheme "http" :host "example.com" :port 80 :path "/" :query nil}

(parse-url "http://localhost:3000/?q=hello")
# → {:scheme "http" :host "localhost" :port 3000 :path "/" :query "q=hello"}

(parse-url "ftp://example.com/")
# → error {:error :http-error :message "parse-url: unsupported scheme in: ftp://example.com/"}
```

## Invariants

1. Header keys are always lowercase keywords after parsing.
2. Content-Length is always set in responses produced by `http-respond`.
3. Connections are always closed via `defer` — never leaked on error.
4. `http-serve` absorbs handler errors (500 response); server keeps running.
5. Only `http://` scheme is supported. No HTTPS, no HTTP/2.
6. Content-Length body only. No chunked transfer encoding.
7. No connection pooling. Each request opens and closes a TCP connection.

## Running tests

```bash
./target/debug/elle tests/elle/http.lisp
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
tls:lines conn → coro/new (loop: tls:read-line → yield)
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
| `tls:lines` | `conn` | coroutine | stream |
| `tls:chunks` | `conn size` | coroutine | stream |
| `tls:writer` | `conn` | coroutine | stream |

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
