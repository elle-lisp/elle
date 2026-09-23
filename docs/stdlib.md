# Standard Library

<!-- audited: 2026-09-23 -->

Elle's standard library has four layers: Rust primitives, core operators,
prelude macros, and stdlib functions.

## Libraries (`lib/`)

Higher-level modules, loaded with `import`. Each module is a closure:
`(import "std/NAME")` returns it, and calling it returns a struct of the
module's exports. Some of them:

| Module | Import | Description |
|--------|--------|-------------|
| aws | `(import "std/aws")` | AWS API client (S3, etc.) |
| contract | `(import "std/contract")` | Design-by-contract assertions |
| dns | `(import "std/dns")` | DNS resolution |
| egui | `(import "std/egui")` | GUI helpers (wraps egui plugin) |
| hash | `(import "std/hash")` | Streaming hash convenience |
| http | `(import "std/http")` | HTTP/1.1 client and server |
| http2 | `(import "std/http2")` | HTTP/2 client and server (h2 + h2c) |
| websocket | `(import "std/websocket")` | WebSocket client and server (RFC 6455) |
| grpc | `(import "std/grpc")` | gRPC client over HTTP/2 |
| lua | `(import "std/lua")` | Lua compat helpers |
| mqtt | `(import "std/mqtt")` | MQTT client wrapper |
| portrait | `(import "std/portrait")` | Semantic portraits |
| process | `(import "std/process")` | Erlang-style processes |
| rdf | `(import "std/rdf/elle")` | RDF knowledge graph |
| redis | `(import "std/redis")` | Redis client |
| sync | `(import "std/sync")` | Synchronization primitives |
| telemetry | `(import "std/telemetry")` | Tracing and metrics |
| tls | `(import "std/tls")` | TLS convenience wrapper |
| watch | `(import "std/watch")` | File watching wrapper |
| zmq | `(import "std/zmq")` | ZeroMQ messaging |

[libraries.md](libraries.md) describes the libraries and how to load them.

## Core operators (`core.lisp`)

Compiled and run before the prelude, from special forms and `%` intrinsics
alone, because prelude macros call them while they expand. They are the
sequence operators and the trait layer those operators dispatch through
([traits.md](traits.md)):

```text
fold reduce reverse append concat last butlast
trait/elements trait/rebuild
```

## Prelude (`prelude.lisp`)

Macros loaded before user code:

```text
defn        function definition sugar
let*        alias for let (sequential bindings)
->  ->>     thread-first, thread-last
as-> some-> some->>   threading variants
when        one-armed conditional
unless      negated one-armed conditional
case        equality dispatch
if-let      conditional binding (two arms)
when-let    conditional binding (one arm)
when-ok     run a body when an expression does not error
each        iteration
repeat      run N times
forever     infinite loop
try/catch   error recovery
protect     error capture
defer       guaranteed cleanup
with        resource management
with-temp-dir   a scratch directory, removed afterwards
error       raise an error
assert      check a condition
gate!       run a body, or raise :gated
yield yield*    emit :yield
apply       call with a spread argument list
default     a default for a &named parameter left nil
```

`cond` and `match` are special forms, not macros.

## stdlib (`stdlib.lisp`)

Functions loaded after the prelude, among them:

```text
map filter sum product take drop
sort-by sort-with
compose partial identity
```

`sort`, `->array`, `->list`, `freeze`, `thaw` and `deep-freeze` are VM
primitives.

## VM primitives

Native functions implemented in Rust. Use `(vm/list-primitives)` to
enumerate, `(doc fn-name)` for documentation, and `(vm/primitive-meta "name")`
for the full metadata struct:

```lisp
(assert (> (length (vm/list-primitives)) 500))
(assert (= (get (vm/primitive-meta "length") :arity) "1"))
(assert (nil? (vm/primitive-meta "+")))   # + is a stdlib function, not a primitive
```

### IEEE 754 bitcast

| Primitive | Arity | Description |
|-----------|-------|-------------|
| `math/f32-bits` | 1 | Return the IEEE 754 f32 bit pattern of a number as an integer |
| `math/f32-from-bits` | 1 | Reinterpret an integer as an IEEE 754 f32 bit pattern |

```lisp
(assert (= (math/f32-bits 1.0) 1065353216))
(assert (= (math/f32-from-bits 1065353216) 1.0))
```

---

## See also

- [plugins.md](plugins.md) — native plugin extensions
- [modules.md](modules.md) — import system
- [functions.md](functions.md) — function reference
