# http

<!-- audited: 2026-11-12 -->

The submodules behind [http.lisp](../../../lib/http.lisp): URLs, transports, the wire format, chunked bodies and server-sent events.

[http.md](../http.md) holds what a caller of the module sees. This file holds
how the module divides. Each file's header says what it exports, and
`(doc name)` carries a function's arguments.

| File | Purpose |
|------|---------|
| [url.lisp](../../../lib/http/url.lisp) | URL parsing and query-string encoding |
| [transport.lisp](../../../lib/http/transport.lisp) | The TCP and TLS transports, each a framer and a gather from [framing](../framing.md) over its connection, that every wire helper reads and writes |
| [chunked.lisp](../../../lib/http/chunked.lisp) | Chunked transfer encoding, read and write |
| [wire.lisp](../../../lib/http/wire.lisp) | Header lines, request and status lines, bodies, reason phrases |
| [sse.lisp](../../../lib/http/sse.lisp) | Server-sent events: the client streams and the response builder |

A submodule takes the submodules it needs as arguments, so loading one by hand
follows the order [http.lisp](../../../lib/http.lisp) uses:

```lisp
(def url ((import "std/http/url")))
(def transport ((import "std/http/transport")))
(def chunked ((import "std/http/chunked") :transport transport))
(def wire ((import "std/http/wire") :transport transport :chunked chunked))
(def sse
  ((import "std/http/sse") :url url :transport transport :wire wire
   :chunked chunked))
(assert (= (url:query-encode {:page 2}) "page=2"))
```

Each submodule carries its own checks as `:test`, and `(http:test)` runs every
one of them.
