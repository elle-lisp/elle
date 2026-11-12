# stdlib

<!-- audited: 2026-11-12 -->

Reusable Elle modules, one closure each: `(import "std/name")` gives you the closure, and calling it returns the struct of exports.

```lisp
(def sync-module (import "std/sync"))
(assert (fn? sync-module))
(def sync (sync-module))
(assert (struct? sync))
(assert (fn? sync:make-lock))
```

A module takes a plugin it depends on as an argument, and so does a
module it lets the caller configure: `std/http` takes `:tls`, and
`std/grpc` takes `:http2`. A module imports the rest of what it needs
itself. The export struct at the bottom of a module's source is the
list of what it offers, and `(doc name)` gives a function its contract.
A guide here therefore carries only what the source cannot: the wire
shapes, the flow between fibers, and the invariants a caller breaks at
its own cost.

## Modules

This table lists the modules with a guide here, and their neighbours.
[docs/libraries.md](../libraries.md) lists every module, with its import.

| File | Purpose | Guide |
|------|---------|-------|
| [http.lisp](../../lib/http.lisp) | HTTP/1.1 client and server over TCP | [http.md](http.md) |
| [http/](http/AGENTS.md) | HTTP/1.1 submodules: url, transport, chunked, wire, sse | [http/overview.md](http/overview.md) |
| [http2.lisp](../../lib/http2.lisp) | HTTP/2 client and server (h2 over TLS, h2c cleartext) | [http2.md](http2.md) |
| [http2/](http2/AGENTS.md) | HTTP/2 submodules: huffman, hpack, frame, stream, transport, session, reader, server | [http2/overview.md](http2/overview.md) |
| [websocket.lisp](../../lib/websocket.lisp) | WebSocket client and server (RFC 6455, ws:// and wss://) | |
| [grpc.lisp](../../lib/grpc.lisp) | gRPC client over HTTP/2 with length-prefixed framing | |
| [tls.lisp](../../lib/tls.lisp) | TLS 1.2/1.3 client and server, with ALPN | [tls.md](tls.md) |
| [redis.lisp](../../lib/redis.lisp) | Redis client (RESP2) over TCP | [redis.md](redis.md) |
| [redis/](redis/AGENTS.md) | Redis submodules: resp, connection, keyspace, collections, zset, server | [redis/overview.md](redis/overview.md) |
| [framing.lisp](../../lib/framing.lisp) | Lines, delimited records and exact counts over any byte source, and a write queue sent in one gathered write per flush | [framing.md](framing.md) |
| [gun.lisp](../../lib/gun.lisp) | Graph database in the shape of GUN: HAM merge, memory and Redis stores, a WebSocket peer mesh, signed user spaces and encryption | [gun.md](gun.md) |
| [gun/](gun/AGENTS.md) | gun submodules: state, value, field, ham, graph, store, wire, sea, core, peer, wait, mesh, join, chain, write | [gun/overview.md](gun/overview.md) |
| [dns.lisp](../../lib/dns.lisp) | DNS client (RFC 1035) | |
| [dns/wire.lisp](../../lib/dns/wire.lisp) | The DNS wire codec: names, queries, responses | |
| [aws.lisp](../../lib/aws.lisp) | AWS client: SigV4 signing, HTTPS, service dispatch | [aws/](aws/AGENTS.md) |
| [contract.lisp](../../lib/contract.lisp) | Compositional validation for function boundaries | |
| [ratchet.lisp](../../lib/ratchet.lisp) | Measure a shape, print the reading, judge it against the ledger | [ratchet.md](ratchet.md) |
| [lua.lisp](../../lib/lua.lisp) | Lua standard library compatibility prelude; fails to compile today ([#1217](https://github.com/elle-lisp/elle/issues/1217)) | |
| [process.lisp](../../lib/process.lisp) | Erlang-style processes, GenServer, Actor, Supervisor | [process.md](process.md) |
| [irc.lisp](../../lib/irc.lisp) | IRCv3 client: CAP negotiation, SASL PLAIN, message tags | [irc.md](irc.md) |
| [sync.lisp](../../lib/sync.lisp) | Lock, semaphore, condvar, rwlock, barrier, latch, once, queue, monitor | |
| [spirv.lisp](../../lib/spirv.lisp) | SPIR-V compute shader emitter | |
| [gpu.lisp](../../lib/gpu.lisp) | GPU compute over the vulkan plugin and the SPIR-V emitter | |
