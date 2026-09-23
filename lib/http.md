# http

<!-- audited: 2026-09-23 -->

HTTP/1.1 client and server over TCP, in one file of pure Elle, with HTTPS and compression as opt-in module arguments.

The export struct at the bottom of [http.lisp](http.lisp) lists what the
module offers, and `(doc name)` carries each function's arguments and
its errors. This file holds what the source cannot: the shapes on the
wire and the invariants a caller has to respect.

## Loading

```lisp
(def http ((import "std/http")))                          # http only
(def http ((import "std/http") :tls ((import "std/tls") (import "plugin/tls"))))
(def http ((import "std/http") :compress true))           # gzip, zlib, deflate, zstd
```

`:compress` exposes the codecs as `http:gzip` and friends. Nothing is
negotiated for you — a caller applies them to a body or to one chunk.

## Data flow

```
client:  request → parse-url → tcp/connect → request line → headers
                 → flush → status line → headers → body → close

server:  serve → tcp/listen → forever:
           accept → ev/spawn → defer(close): read → handler → write
```

Every wire helper works on a **transport**, a struct of closures
`{:read :read-line :write :flush :close}`. `tcp-transport` and
`tls-transport` build one, and the parsed URL's scheme picks which. So
the chunked reader, the body reader and the header code are written once
and serve both schemes. A session from `http:connect` carries
`:transport`, not a bare port.

## Struct shapes

```lisp
# request, handed to a handler
{:method "GET" :path "/foo" :version "HTTP/1.1"
 :headers {:host "example.com"} :body "..."}

# response, returned by a handler or by http:respond
{:status 200 :headers {:content-type "text/plain"} :body "hello"}

# url, from parse-url
{:scheme "http" :host "example.com" :port 80 :path "/foo" :query "page=1"}
```

Server-sent events ride the chunked path: `sse-response` sets
`text/event-stream`, and each `send-event` call becomes one chunk.
`sse-get` answers with a `|:yield|` fiber of `{:event :data :id :retry}`
structs that reconnects on its own.

## Invariants

1. Header keys are lowercase keywords after parsing.
2. `http:respond` always sets Content-Length.
3. `defer` closes every connection, so an error never leaks one.
4. `http:serve` answers a handler error with 500 and keeps serving.
5. `https://` needs `:tls` at module init, and signals
   `:http-error :tls-not-configured` without it.
6. `http:serve` speaks plain HTTP. For HTTPS, wrap an accepted
   connection with `tls:accept` and feed `tls-transport` to
   `connection-loop`.
7. `Transfer-Encoding: chunked` wins over `Content-Length`.
8. No connection pooling. Each one-shot call opens and closes a transport.

## Running tests

```bash
elle tests/elle/http.lisp
```
