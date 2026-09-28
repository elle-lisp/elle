# http

<!-- audited: 2026-09-28 -->

The submodules behind [http.lisp](../http.lisp): URLs, transports, the wire format, chunked bodies and server-sent events.

[http.md](../http.md) holds what a caller of the module sees. This file holds
how the module divides. Each file's header says what it exports, and
`(doc name)` carries a function's arguments.

| File | Purpose |
|------|---------|
| [url.lisp](url.lisp) | URL parsing and query-string encoding |
| [transport.lisp](transport.lisp) | The TCP and TLS transports that every wire helper reads and writes |
| [chunked.lisp](chunked.lisp) | Chunked transfer encoding, read and write |
| [wire.lisp](wire.lisp) | Header lines, request and status lines, bodies, reason phrases |
| [sse.lisp](sse.lisp) | Server-sent events: the client streams and the response builder |

A submodule takes the submodules it needs as arguments, so loading one by hand
follows the order [http.lisp](../http.lisp) uses:

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
