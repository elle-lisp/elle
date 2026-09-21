# Libraries

<!-- audited: 2026-09-23 -->

Elle ships with libraries in `lib/`. All follow the closure-as-module
pattern and are imported via `(import "std/<name>")`.

## Networking

| Module | Import | Description |
|--------|--------|-------------|
| http | `(import "std/http")` | Pure Elle HTTP/1.1 client and server |
| http2 | `(import "std/http2")` | HTTP/2 client and server (h2 over TLS, h2c cleartext) |
| tls | `(import "std/tls")` | TLS client and server (wraps tls plugin, ALPN support) |
| dns | `(import "std/dns")` | Pure Elle DNS client (RFC 1035) |
| aws | `(import "std/aws")` | Elle-native AWS client (S3, etc.) |
| redis | `(import "std/redis")` | Pure Elle Redis client (RESP2) |
| irc | `(import "std/irc")` | Coroutine-based IRCv3 client with SASL |
| mqtt | `(import "std/mqtt")` | MQTT client (wraps mqtt plugin) |
| zmq | `(import "std/zmq")` | ZeroMQ bindings via FFI |
| websocket | `(import "std/websocket")` | WebSocket client and server (RFC 6455, ws:// and wss://) |
| grpc | `(import "std/grpc")` | gRPC client over HTTP/2 |

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
| resource | `(import "std/resource")` | Deterministic resource consumption measurement |

## GPU and Graphics

| Module | Import | Description |
|--------|--------|-------------|
| gpu | `(import "std/gpu")` | GPU compute via MLIR → SPIR-V → Vulkan (`gpu:map`); needs a build with `--features mlir` |
| spirv | `(import "std/spirv")` | Hand-written SPIR-V compute shader DSL |
| gtk4 | `(import "std/gtk4")` | GTK4 bindings via FFI (declarative widgets, WebKit) |
| sdl3 | `(import "std/sdl3")` | SDL3 bindings via FFI (events, textures, audio, TTF) |
| raylib | `(import "std/raylib")` | raylib bindings via FFI |
| cairo | `(import "std/cairo")` | Cairo 2D drawing via FFI |
| wayland | `(import "std/wayland")` | Wayland compositor interaction (wraps wayland plugin) |

## Utilities

| Module | Import | Description |
|--------|--------|-------------|
| hash | `(import "std/hash")` | Streaming hash convenience functions |
| watch | `(import "std/watch")` | Event-driven filesystem watcher (inotify/kqueue) |
| color | `(import "std/color")` | Color spaces, mixing, gradients, perceptual distance |
| egui | `(import "std/egui")` | Immediate-mode GUI (wraps egui plugin) |
| lua | `(import "std/lua")` | Lua compatibility prelude; fails to compile today (#1217) |
| svg | `(import "std/svg")` | SVG construction and emission (pure Elle) |

### Utilities (pure Elle / FFI)

| Module | Import | Description |
|--------|--------|-------------|
| base64 | `((import "std/base64"))` | Base64 encoding/decoding |
| cli | `((import "std/cli"))` | CLI argument parsing |
| compress | `((import "std/compress"))` | Gzip, zlib, deflate, zstd (FFI to libz + libzstd) |
| git | `((import "std/git"))` | Git repository operations (FFI to libgit2) |
| glob | `((import "std/glob"))` | Filesystem glob pattern matching |
| semver | `((import "std/semver"))` | Semantic version parsing and comparison |
| semver/diff | `((import "std/semver/diff"))` | Surface diffs, bump floors, claim verdicts. See [semver.md](semver.md) |
| semver/file | `((import "std/semver/file"))` | `.surface` file rendering and parsing |
| semver/surface | `((import "std/semver/surface"))` | Public-surface extraction from a module |
| sqlite | `((import "std/sqlite"))` | SQLite database (FFI to libsqlite3) |
| uuid | `((import "std/uuid"))` | UUID generation and parsing |

## Usage

A library is a closure. Import it and call the closure for its exports:

```lisp
(def http ((import "std/http")))
(def [ok? err] (protect (http:get "https://example.com")))
(assert (= :tls-not-configured (get err :reason)) "https needs :tls")
```

A library that depends on a native plugin takes the plugin as an argument.
HTTPS takes the `std/tls` module, built from the plugin that
`(import "plugin/tls")` loads, as `:tls`:

```lisp
(defn https-client [tls-plugin]
  "An HTTP module that can fetch https:// URLs."
  (let [tls ((import "std/tls") tls-plugin)]
    ((import "std/http") :tls tls)))
```

See [modules.md](modules.md) for how the module system works and
[plugins.md](plugins.md) for native plugins.
