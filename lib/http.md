# http

<!-- audited: 2026-09-23 -->

HTTP/1.1 client and server over TCP, in one file of pure Elle, with HTTPS and compression as opt-in module arguments.

The export struct at the bottom of [http.lisp](http.lisp) lists what the
module offers, and `(doc name)` carries each function's arguments and
its errors. This file holds what the source cannot: the shapes on the
wire and the invariants a caller has to respect.

## Loading

Called with no argument, the module speaks plain HTTP. HTTPS takes the
`std/tls` module built from the plugin, `(import "plugin/tls")`, as
`:tls`, and `:compress true` loads the codecs, which need `libz` and
`libzstd`:

```lisp
(def http ((import "std/http")))                          # http only

(defn https-module [tls-plugin]
  "The http module, able to fetch https:// URLs."
  ((import "std/http") :tls ((import "std/tls") tls-plugin)))

(defn http-with-codecs []
  "The http module with gzip, zlib, deflate and zstd."
  ((import "std/http") :compress true))
```

## Data flow

```
client:  request → parse-url → open transport → request line → headers
                 → flush → status line → headers → body → close

server:  tcp/listen → serve → forever:
           accept → ev/spawn → defer(close): read → handler → write
```

Every wire helper works on a **transport**, a struct of closures
`{:read :read-line :write :flush :close}`. `tcp-transport` and
`tls-transport` build one, and the parsed URL's scheme picks which. So
the chunked reader, the body reader and the header code are written once
and serve both schemes. A session from `http:connect` carries
`:transport`, not a bare port.

## Struct shapes

A handler receives a request. `:body` is nil when the request carries
neither `Content-Length` nor chunked framing:

```text
{:method "GET" :path "/foo" :version "HTTP/1.1"
 :headers {:host "example.com"} :body "..."}
```

A handler returns a response, and `http:respond` builds one with
`Content-Type` and `Content-Length` set. `http:parse-url` answers the
URL shape:

```lisp
(assert (= (http:respond 200 "hello")
           {:status 200
            :headers {:content-type "text/plain" :content-length "5"}
            :body "hello"}))
(assert (= (http:parse-url "http://example.com/foo?page=1")
           {:scheme "http" :host "example.com" :port 80 :path "/foo"
            :query "page=1"}))
```

Server-sent events ride the chunked path: `sse-response` sets
`text/event-stream`, and each call to the `send-event` function it hands
the body becomes one chunk. `sse-get` answers with a `|:yield|` fiber of
`{:event :data :id :retry}` structs that reconnects on its own.

## Invariants

1. Header keys are lowercase keywords after parsing.
2. `http:respond` always sets Content-Length.
3. `defer` closes every connection, so an error never leaks one.
4. `http:serve` answers a handler error with 500 and keeps serving.
5. `https://` needs `:tls` at module init, and signals
   `:http-error :tls-not-configured` without it.
6. `http:serve` speaks plain HTTP, and the module exports no HTTPS
   server: the loop that serves one transport is private.
7. `Transfer-Encoding: chunked` wins over `Content-Length`.
8. No connection pooling. Each one-shot call opens and closes a transport.

## Running tests

```bash
elle tests/elle/http.lisp
```
