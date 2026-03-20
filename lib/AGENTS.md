# lib/http

Agent guide for `lib/http.lisp` — Pure Elle HTTP/1.1 client and server.

## Purpose

HTTP/1.1 over TCP using Elle's existing stream and scheduler primitives.
Single file. No Rust changes (other than `port/path`, added in Chunk 0).

## Data flow

Client:
```
http-get url → parse-url → tcp/connect → write-request-line → write-headers
→ stream/flush → read-status-line → read-headers → read-body → port/close → response
```

Server:
```
http-serve port handler → tcp/listen → ev/run → forever:
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
elle tests/elle/http.lisp
```

---

# lib/redis

Agent guide for `lib/redis.lisp` — Pure Elle Redis client (RESP2 over TCP).

## Purpose

Redis client speaking RESP2 over TCP using Elle's stream and scheduler primitives.
Single file. No Rust changes. No external dependencies.

## Loading

```lisp
(def redis ((import-file "lib/redis.lisp")))
```

## Connection

A connection is `{:port <tcp-port> :type :redis-conn}`.

```lisp
(redis:connect host port)                  → conn
(redis:connect host port :auth "pass")     → conn
(redis:connect host port :db 2)            → conn
(redis:close conn)                         → nil
(redis:command conn ["CMD" "arg" ...])     → value
```

## Exported functions

| Function | Signature | Returns |
|----------|-----------|---------|
| `connect` | `(fn [host port &named auth db])` | conn |
| `close` | `(fn [conn])` | nil |
| `command` | `(fn [conn args])` | RESP value |
| `get` | `(fn [conn key])` | string or nil |
| `set` | `(fn [conn key val &named ex px nx xx])` | true or nil |
| `del` | `(fn [conn key])` | int |
| `exists` | `(fn [conn key])` | bool |
| `incr` / `decr` | `(fn [conn key])` | int |
| `expire` | `(fn [conn key seconds])` | bool |
| `ttl` | `(fn [conn key])` | int |
| `mget` | `(fn [conn keys])` | array |
| `mset` | `(fn [conn pairs])` | true |
| `lpush` / `rpush` | `(fn [conn key val])` | int |
| `lpop` / `rpop` | `(fn [conn key])` | string or nil |
| `lrange` | `(fn [conn key start stop])` | array |
| `llen` | `(fn [conn key])` | int |
| `hset` | `(fn [conn key field val])` | int |
| `hget` | `(fn [conn key field])` | string or nil |
| `hgetall` | `(fn [conn key])` | struct (string keys) |
| `hdel` | `(fn [conn key field])` | int |
| `hexists` | `(fn [conn key field])` | bool |
| `hlen` | `(fn [conn key])` | int |
| `hmset` | `(fn [conn key pairs])` | true |
| `hmget` | `(fn [conn key fields])` | array |
| `sadd` / `srem` | `(fn [conn key member])` | int |
| `smembers` | `(fn [conn key])` | array |
| `scard` | `(fn [conn key])` | int |
| `sismember` | `(fn [conn key member])` | bool |
| `zadd` | `(fn [conn key score member])` | int |
| `zrange` | `(fn [conn key start stop])` | array |
| `zrank` | `(fn [conn key member])` | int or nil |
| `zscore` | `(fn [conn key member])` | string or nil |
| `zrem` | `(fn [conn key member])` | int |
| `zcard` | `(fn [conn key])` | int |
| `keys` | `(fn [conn pattern])` | array |
| `flushdb` | `(fn [conn])` | true |
| `ping` | `(fn [conn])` | "PONG" |
| `select` | `(fn [conn db])` | true |
| `auth` | `(fn [conn password])` | true |
| `dbsize` | `(fn [conn])` | int |
| `info` | `(fn [conn])` | string |
| `publish` | `(fn [conn channel msg])` | int |
| `subscribe` | `(fn [conn channel handler])` | nil |
| `psubscribe` | `(fn [conn pattern handler])` | nil |
| `pipeline` | `(fn [conn commands])` | array |
| `test` | `(fn [])` | true |

## Value mapping

- Redis `OK` → `true`
- Redis nil → Elle `nil`
- Redis integer → Elle int
- Redis array → immutable array
- Redis error → signals `:redis-error` (or struct in pipeline mode)
- `HGETALL` → struct with string keys `{"field" "value"}`
- `EXISTS`/`HEXISTS`/`SISMEMBER`/`EXPIRE` → boolean

## Running tests

```bash
# Internal RESP tests (no Redis needed):
elle -c '(def r ((import-file "lib/redis.lisp"))) (r:test) (print "ok")'

# Integration tests (requires Redis on localhost:6379):
elle tests/elle/redis.lisp
```
