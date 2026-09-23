# lib

<!-- audited: 2026-09-23 -->

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
[docs/libraries.md](../docs/libraries.md) lists every module, with its import.

| File | Purpose | Guide |
|------|---------|-------|
| [http.lisp](http.lisp) | HTTP/1.1 client and server over TCP | [http.md](http.md) |
| [http2.lisp](http2.lisp) | HTTP/2 client and server (h2 over TLS, h2c cleartext) | [http2.md](http2.md) |
| [http2/](http2/AGENTS.md) | HTTP/2 submodules: huffman, hpack, frame, stream, transport, session, reader, server | [http2/overview.md](http2/overview.md) |
| [websocket.lisp](websocket.lisp) | WebSocket client and server (RFC 6455, ws:// and wss://) | |
| [grpc.lisp](grpc.lisp) | gRPC client over HTTP/2 with length-prefixed framing | |
| [tls.lisp](tls.lisp) | TLS 1.2/1.3 client and server, with ALPN | [tls.md](tls.md) |
| [redis.lisp](redis.lisp) | Redis client (RESP2) over TCP | [redis.md](redis.md) |
| [dns.lisp](dns.lisp) | DNS client (RFC 1035) | |
| [aws.lisp](aws.lisp) | AWS client: SigV4 signing, HTTPS, service dispatch | [aws/](aws/AGENTS.md) |
| [contract.lisp](contract.lisp) | Compositional validation for function boundaries | |
| [lua.lisp](lua.lisp) | Lua standard library compatibility prelude; fails to compile today ([#1217](https://github.com/elle-lisp/elle/issues/1217)) | |
| [process.lisp](process.lisp) | Erlang-style processes, GenServer, Actor, Supervisor | [process.md](process.md) |
| [irc.lisp](irc.lisp) | IRCv3 client: CAP negotiation, SASL PLAIN, message tags | [irc.md](irc.md) |
| [sync.lisp](sync.lisp) | Lock, semaphore, condvar, rwlock, barrier, latch, once, queue, monitor | |
| [spirv.lisp](spirv.lisp) | SPIR-V compute shader emitter | |
| [gpu.lisp](gpu.lisp) | GPU compute over the vulkan plugin and the SPIR-V emitter | |
