# Standard Library

Elle's standard library has three layers: VM primitives (Rust), stdlib
functions (Elle), and prelude macros (Elle).

## Libraries (`lib/`)

Higher-level modules loaded with `import-file`. Each wraps its code in
a closure returning a struct.

| Module | Import | Description |
|--------|--------|-------------|
| `lib/aws.lisp` | `(import-file "lib/aws.lisp")` | AWS API client (S3, etc.) |
| `lib/contract.lisp` | `(import-file "lib/contract.lisp")` | Design-by-contract assertions |
| `lib/dns.lisp` | `(import-file "lib/dns.lisp")` | DNS resolution |
| `lib/egui.lisp` | `(import-file "lib/egui.lisp")` | GUI helpers (wraps egui plugin) |
| `lib/hash.lisp` | `(import-file "lib/hash.lisp")` | Streaming hash convenience |
| `lib/http.lisp` | `(import-file "lib/http.lisp")` | HTTP client |
| `lib/lua.lisp` | `(import-file "lib/lua.lisp")` | Lua compat helpers |
| `lib/mqtt.lisp` | `(import-file "lib/mqtt.lisp")` | MQTT client wrapper |
| `lib/portrait.lisp` | `(import-file "lib/portrait.lisp")` | Semantic portraits |
| `lib/process.lisp` | `(import-file "lib/process.lisp")` | Erlang-style processes |
| `lib/rdf.lisp` | `(import-file "lib/rdf.lisp")` | RDF knowledge graph |
| `lib/redis.lisp` | `(import-file "lib/redis.lisp")` | Redis client |
| `lib/sync.lisp` | `(import-file "lib/sync.lisp")` | Synchronization primitives |
| `lib/telemetry.lisp` | `(import-file "lib/telemetry.lisp")` | Tracing and metrics |
| `lib/tls.lisp` | `(import-file "lib/tls.lisp")` | TLS convenience wrapper |
| `lib/watch.lisp` | `(import-file "lib/watch.lisp")` | File watching wrapper |
| `lib/zmq.lisp` | `(import-file "lib/zmq.lisp")` | ZeroMQ messaging |

## Prelude (`prelude.lisp`)

Macros loaded before user code. These define fundamental control flow:

```text
defn        function definition sugar
let*        sequential bindings
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

---

## See also

- [plugins.md](plugins.md) — native plugin extensions
- [modules.md](modules.md) — import system
- [functions.md](functions.md) — function reference
