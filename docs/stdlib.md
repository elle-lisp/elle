# Standard Library

Elle's standard library has three layers: VM primitives (Rust), stdlib
functions (Elle), and prelude macros (Elle).

## Libraries (`lib/`)

Higher-level modules loaded with `import-file`. Each wraps its code in
a closure returning a struct.

| Module | Import | Description |
|--------|--------|-------------|
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

## Prelude (`prelude.lisp`)

Macros loaded before user code. These define fundamental control flow:

```text
defn        function definition sugar
let*        alias for let (sequential bindings)
->          thread-first
->>         thread-last
when        one-armed conditional
unless      negated one-armed conditional
cond        multi-branch conditional
case        equality dispatch
if-let      conditional binding (two arms)
when-let    conditional binding (one arm)
while-let   conditional loop
match       pattern matching
each        iteration
try/catch   error recovery
protect     error capture
defer       guaranteed cleanup
with        resource management
repeat      run N times
forever     infinite loop
error       raise an error
```

## stdlib (`stdlib.lisp`)

Functions loaded after the prelude:

```text
map filter fold apply sum product
append reverse take drop butlast last
sort sort-by sort-with
compose partial identity
->array ->list
freeze thaw deep-freeze
```

## VM primitives

Native functions implemented in Rust. Use `(vm/list-primitives)` to
enumerate, or `(doc fn-name)` for documentation.

```text
(doc +)                    # shows arity, params, examples
(vm/primitive-meta "+")    # returns full metadata struct
```

### IEEE 754 bitcast

| Primitive | Arity | Description |
|-----------|-------|-------------|
| `math/f32-bits` | 1 | Return the IEEE 754 f32 bit pattern of a number as an integer |
| `math/f32-from-bits` | 1 | Reinterpret an integer as an IEEE 754 f32 bit pattern |

---

## See also

- [plugins.md](plugins.md) — native plugin extensions
- [modules.md](modules.md) — import system
- [functions.md](functions.md) — function reference
