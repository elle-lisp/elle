# http2

<!-- audited: 2026-09-14 -->

HTTP/2 client and server (RFC 9113 + RFC 7541), over TLS with ALPN or as cleartext h2c.

The export struct at the bottom of [http2.lisp](http2.lisp) lists what
the module offers, and `(doc name)` carries each function's arguments.
The codecs and the session machinery live in [http2/](http2/AGENTS.md),
which also holds the design decisions and the invariants.

## Loading

```lisp
(def http2 ((import "std/http2")))                        # h2c
(def tls   ((import "std/tls") (import "plugin/tls")))
(def http2 ((import "std/http2") :tls tls))               # h2 over TLS
```

## Struct shapes

```lisp
# response
{:status 200 :headers {:content-type "text/html"} :body <bytes>}

# session
@{:transport :is-server? :streams :next-stream-id :hpack-encoder
  :hpack-decoder :local-settings :remote-settings :conn-flow
  :write-queue :reader-fiber :writer-fiber :closed? :host}
```

## Running tests

```bash
elle tests/elle/http2.lisp
elle tests/elle/h2-close-on-dead-peer.lisp
elle tests/http2/all.lisp
elle tests/http2/flow.lisp
```
