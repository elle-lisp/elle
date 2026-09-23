# http2

<!-- audited: 2026-09-23 -->

HTTP/2 client and server (RFC 9113 + RFC 7541), over TLS with ALPN or as cleartext h2c.

The export struct at the bottom of [http2.lisp](http2.lisp) lists what
the module offers, and `(doc name)` carries each function's arguments.
The codecs and the session machinery live in
[http2/](http2/overview.md), which also holds the design decisions and
the invariants.

## Loading

Called with no argument, the module speaks h2c. For h2 over TLS, pass
the `std/tls` module built from the plugin, `(import "plugin/tls")`, as
`:tls`:

```lisp
(def http2 ((import "std/http2")))                        # h2c

(defn http2-over-tls [tls-plugin]
  "The http2 module, speaking h2 over TLS."
  ((import "std/http2") :tls ((import "std/tls") tls-plugin)))
```

## Struct shapes

```text
# response, from http2:send and the one-shot calls
{:status 200 :headers {:content-type "text/html"} :trailers {} :body <bytes>}

# session, from http2:connect
@{:transport :is-server? :host :scheme :streams :next-stream-id
  :hpack-encoder :hpack-decoder :local-settings :remote-settings
  :conn-flow :write-queue :reader-fiber :writer-fiber :closed?
  :goaway-recvd? :last-stream-id :settings-ack-latch :pending-conn-wu
  :wu-threshold :expecting-continuation-sid}
```

## Running tests

```bash
elle tests/elle/http2.lisp
elle tests/elle/h2-close-on-dead-peer.lisp
elle tests/http2/all.lisp
elle tests/http2/flow.lisp
```
