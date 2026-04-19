# webserver

Demo HTTP server and concurrent load generator.

## Responsibility

Demonstrate Elle's async I/O, HTTP library, and concurrency primitives
in a realistic network application. Exercises `std/http` (client and
server), `ev/map-limited` for bounded concurrency, `ev/sleep` for
timed delays, and `clock/monotonic` for latency measurement.

Does NOT:
- Use TLS/HTTPS (plain HTTP only)
- Require any Rust plugins
- Require external dependencies beyond Elle itself

## Key files

| File | Purpose |
|------|---------|
| `server.lisp` | HTTP server with 6 endpoints (health, echo, delay, counter, stats) |
| `loadgen.lisp` | Concurrent load generator with percentile latency stats (fresh + keepalive modes) |
| `bench.lisp` | Concurrency sweep with SVG chart generation via `plugin/plotters` |

## Dependencies

- `std/http` via `(import "std/http")`
- `plugin/plotters` via `(import "plugin/plotters")` (bench.lisp only)

## Known issues

Keep-alive connections via `connection-loop` have a 150x latency
regression vs equivalent raw TCP ops (40ms vs 0.26ms). See README.md
for measurements and `.claude/plans/keepalive-regression.md` for the
investigation plan.

## Running

```bash
# Terminal 1: start server
elle demos/webserver/server.lisp 8080

# Terminal 2: run load test
elle demos/webserver/loadgen.lisp http://127.0.0.1:8080/ 100 10
```

## Server endpoints

| Route | Method | Behavior |
|-------|--------|----------|
| `/` | GET | Welcome message (plain text) |
| `/health` | GET | `{"status":"ok"}` JSON |
| `/echo` | POST | Echo request body |
| `/delay/:ms` | GET | Sleep ms then respond |
| `/counter` | GET | Incrementing hit counter |
| `/stats` | GET | JSON uptime + request count |
| `*` | `*` | 404 |
