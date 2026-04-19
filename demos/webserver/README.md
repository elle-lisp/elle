# Web Server + Load Generator

A demo HTTP server and concurrent load generator written in Elle.

## Quick start

```bash
# Start the server (default port 8080)
elle demos/webserver/server.lisp

# In another terminal, run the load generator
elle demos/webserver/loadgen.lisp http://127.0.0.1:8080/ 1000 50
```

## Server

`server.lisp` is a multi-endpoint HTTP server built on `std/http`:

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/` | GET | Welcome page |
| `/health` | GET | JSON health check |
| `/echo` | POST | Echoes the request body |
| `/delay/:ms` | GET | Responds after sleeping for `:ms` milliseconds |
| `/counter` | GET | Returns an incrementing hit counter |
| `/stats` | GET | JSON with server uptime and total request count |

The port defaults to 8080 and can be overridden via CLI argument:

```bash
elle demos/webserver/server.lisp 9090
```

## Load generator

`loadgen.lisp` fires concurrent HTTP requests and reports latency
statistics. Two modes:

- **fresh** (default) — new TCP connection per request; realistic baseline
- **keepalive** — persistent connections reused across requests

```bash
elle demos/webserver/loadgen.lisp [url] [requests] [concurrency] [keepalive]
```

| # | Parameter | Default |
|---|-----------|---------|
| 1 | Target URL | `http://127.0.0.1:8080/` |
| 2 | Total requests | 1000 |
| 3 | Concurrency | 50 |
| 4 | `keepalive` (literal) | fresh connections |

## Benchmarking

`bench.lisp` sweeps concurrency levels and generates SVG charts via
the `plugin/plotters` plugin:

```bash
elle demos/webserver/bench.lisp http://127.0.0.1:8080/
```

Produces three charts:

### Throughput vs concurrency

![throughput](throughput.svg)

Peak throughput is ~2500 req/s at c=5–25, dropping at higher concurrency
as scheduler contention grows.

### Latency percentiles vs concurrency

![latency](latency.svg)

p50/p95/p99 scale roughly linearly with concurrency. The tight spread
between percentiles means tail latency tracks the median — no outlier
storms.

### Latency distribution at peak concurrency

![histogram](histogram.svg)

Bell curve centered around 45ms at c=100, with a small tail to ~60ms.

## Known issue: keepalive latency regression

Keep-alive connections through `http:serve`'s `connection-loop` exhibit
a 150x latency regression compared to equivalent raw TCP operations:

| Path | Latency per request |
|------|---------------------|
| Raw TCP (1 write + 1 read per side) | 0.26ms |
| Transport-wrapped (same ops) | 0.26ms |
| HTTP string parsing alone | 0.001ms |
| `protect` overhead | 0.003ms |
| **`connection-loop` via `http:serve`** | **40ms** |

The regression is not in HTTP parsing, `protect`, transport wrapping,
or the number of I/O operations — all of these were individually
measured at sub-millisecond cost. The overhead appears when the same
operations run inside `connection-loop`'s nested `defer`/`protect`/
`forever`/`break` control flow, suggesting a runtime interaction
between fiber scheduling and deeply nested control forms.

Fresh connections are unaffected (~0.4ms per request) because each
request runs in an independent fiber without the keep-alive loop
overhead.

See the investigation plan at `.claude/plans/keepalive-regression.md`.

## Features demonstrated

- `std/http` server and client
- `ev/map-limited` for bounded concurrency
- `ev/sleep` for async delays
- `clock/monotonic` for high-resolution timing
- `json/serialize` for JSON responses
- `protect` for error capture without propagation
- `plugin/plotters` for SVG chart generation
