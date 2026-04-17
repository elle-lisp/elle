# Libraries

Elle ships with libraries in `lib/`. All follow the closure-as-module
pattern and are imported via `(import "std/<name>")`.

## Networking

| Module | Import | Description |
|--------|--------|-------------|
| http | `(import "std/http")` | Pure Elle HTTP/1.1 client and server |
| tls | `(import "std/tls")` | TLS client and server (wraps tls plugin) |
| dns | `(import "std/dns")` | Pure Elle DNS client (RFC 1035) |
| aws | `(import "std/aws")` | Elle-native AWS client (S3, etc.) |
| redis | `(import "std/redis")` | Pure Elle Redis client (RESP2) |
| mqtt | `(import "std/mqtt")` | MQTT client (wraps mqtt plugin) |
| zmq | `(import "std/zmq")` | ZeroMQ bindings via FFI |

## Concurrency

| Module | Import | Description |
|--------|--------|-------------|
| sync | `(import "std/sync")` | Locks, semaphores, condvars, rwlocks, barriers, latches, queues |
| process | `(import "std/process")` | Erlang-style processes: GenServer, Supervisor, Actor, Task, EventManager. See [processes.md](processes.md) |

## Analysis

| Module | Import | Description |
|--------|--------|-------------|
| portrait | `(import "std/portrait")` | Semantic portraits from compile/analyze: signal profiles, phases, composition |
| contract | `(import "std/contract")` | Compositional validation for function boundaries |
| rdf | `(import "std/rdf/elle")` | RDF triple generation for the Elle knowledge graph |

## Observability

| Module | Import | Description |
|--------|--------|-------------|
| telemetry | `(import "std/telemetry")` | OpenTelemetry metrics (OTLP/HTTP JSON export) |

## Utilities

| Module | Import | Description |
|--------|--------|-------------|
| hash | `(import "std/hash")` | Streaming hash convenience functions |
| watch | `(import "std/watch")` | Event-driven filesystem watcher (inotify/kqueue) |
| egui | `(import "std/egui")` | Immediate-mode GUI (wraps egui plugin) |
| lua | `(import "std/lua")` | Lua compatibility prelude |

### Utilities (pure Elle / FFI)

| Module | Import | Description |
|--------|--------|-------------|
| base64 | `((import "std/base64"))` | Base64 encoding/decoding |
| cli | `((import "std/cli"))` | CLI argument parsing |
| compress | `((import "std/compress"))` | Gzip, zlib, deflate, zstd (FFI to libz + libzstd) |
| git | `((import "std/git"))` | Git repository operations (FFI to libgit2) |
| glob | `((import "std/glob"))` | Filesystem glob pattern matching |
| semver | `((import "std/semver"))` | Semantic version parsing and comparison |
| sqlite | `((import "std/sqlite"))` | SQLite database (FFI to libsqlite3) |
| uuid | `((import "std/uuid"))` | UUID generation and parsing |

## Usage

Libraries are parametric modules. Import and call the closure:

```text
(def http ((import "std/http")))
(http:get "https://example.com")
```

Libraries that depend on native plugins take the plugin as a parameter:

```text
(def tls-plugin (import "plugin/tls"))
(def tls ((import "std/tls") tls-plugin))
(tls:connect "example.com" 443)
```

See [modules.md](modules.md) for how the module system works and
[plugins.md](plugins.md) for native plugins.
