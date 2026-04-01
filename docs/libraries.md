# Libraries

Elle ships with libraries in `lib/`. All follow the closure-as-module
pattern and are imported via `(import "lib/<name>")`.

## Networking

| Module | Import | Description |
|--------|--------|-------------|
| http | `(import "lib/http")` | Pure Elle HTTP/1.1 client and server |
| tls | `(import "lib/tls")` | TLS client and server (wraps tls plugin) |
| dns | `(import "lib/dns")` | Pure Elle DNS client (RFC 1035) |
| aws | `(import "lib/aws")` | Elle-native AWS client (S3, etc.) |
| redis | `(import "lib/redis")` | Pure Elle Redis client (RESP2) |
| mqtt | `(import "lib/mqtt")` | MQTT client (wraps mqtt plugin) |
| zmq | `(import "lib/zmq")` | ZeroMQ bindings via FFI |

## Concurrency

| Module | Import | Description |
|--------|--------|-------------|
| sync | `(import "lib/sync")` | Locks, semaphores, condvars, rwlocks, barriers, latches, queues |
| process | `(import "lib/process")` | Erlang-style processes: GenServer, Supervisor, Actor, Task, EventManager. See [processes.md](processes.md) |

## Analysis

| Module | Import | Description |
|--------|--------|-------------|
| portrait | `(import "lib/portrait")` | Semantic portraits from compile/analyze: signal profiles, phases, composition |
| contract | `(import "lib/contract")` | Compositional validation for function boundaries |
| rdf | `(import "lib/rdf")` | RDF triple generation for the Elle knowledge graph |

## Observability

| Module | Import | Description |
|--------|--------|-------------|
| telemetry | `(import "lib/telemetry")` | OpenTelemetry metrics (OTLP/HTTP JSON export) |

## Utilities

| Module | Import | Description |
|--------|--------|-------------|
| hash | `(import "lib/hash")` | Streaming hash convenience functions |
| watch | `(import "lib/watch")` | Event-driven filesystem watcher (wraps watch plugin) |
| egui | `(import "lib/egui")` | Immediate-mode GUI (wraps egui plugin) |
| lua | `(import "lib/lua")` | Lua compatibility prelude |

## Usage

Libraries are parametric modules. Import and call the closure:

```text
(def http ((import "lib/http")))
(http:get "https://example.com")
```

Libraries that depend on native plugins take the plugin as a parameter:

```text
(def tls-plugin (import "plugin/tls"))
(def tls ((import "lib/tls") tls-plugin))
(tls:connect "example.com" 443)
```

See [modules.md](modules.md) for how the module system works and
[plugins.md](plugins.md) for native plugins.
