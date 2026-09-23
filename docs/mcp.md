# MCP Server

<!-- audited: 2026-09-23 -->

The Elle MCP server gives a coding assistant structured access to an Elle codebase over the Model Context Protocol.

The [MCP](https://modelcontextprotocol.io) server is written in Elle and maintained in a
[separate repository](https://github.com/elle-lisp/mcp), included as a
git submodule under `mcp/`. It communicates via JSON-RPC 2.0 on stdio.

**See also:** [Agent Reasoning in Elle](analysis/agent-reasoning.md) for how to use MCP + portrait together. [Portrait](analysis/portrait.md) for local file analysis. [Analysis directory](analysis/) for an overview of code understanding tools. [The eval tool](mcp-eval.md) for its contract.

## What it does

The server maintains a persistent RDF knowledge graph (via the oxigraph
plugin) that represents the structure of both Elle and Rust source code.
It exposes 21 tools that let an AI agent query, analyze, evaluate,
refactor code through the graph, and orchestrate test runs with result
tracking.

## Tools

### Graph management

| Tool | Description |
|------|-------------|
| `ping` | Verify the server is alive |
| `sparql_query` | Execute SPARQL SELECT / ASK / CONSTRUCT against the knowledge graph |
| `sparql_update` | Execute SPARQL UPDATE (INSERT DATA, DELETE, etc.) |
| `load_rdf` | Load RDF data from a string (turtle, ntriples, nquads, rdfxml) |
| `dump_rdf` | Serialize the knowledge graph to a string |

### Static analysis

| Tool | Description |
|------|-------------|
| `analyze_file` | Analyze an Elle source file — reports symbols, signals, diagnostics, and observations. Loads the file's functions, signals, captures, composition properties and call edges into the graph. |
| `portrait` | Semantic portrait of a function or module. Shows the effect profile (silent/yields/errors), failure modes, composition properties, and human-readable observations. Omit the function name for a module-level portrait. |
| `signal_query` | Find functions matching a signal property: `silent`, `io`, `yields`, `jit-eligible`, `errors`, or any registered signal keyword. |
| `impact` | Assess the impact of changing a function. Shows its signal, each caller with a warning when the caller is silent or JIT-eligible, its callees, and its captures. |

### Refactoring

| Tool | Description |
|------|-------------|
| `compile_rename` | Binding-aware rename of a function or variable and all its references. Understands lexical scope — won't rename shadowed bindings. |
| `compile_extract` | Extract a line range into a new function. Computes free variables (which become parameters) and infers the extracted function's signal. |
| `compile_parallelize` | Check if a set of functions can safely run in parallel. Verifies no shared mutable captures. |

### Evaluation

| Tool | Description |
|------|-------------|
| `eval` | Evaluate an Elle lambda against the persistent image. Returns a handle (UUID) naming the result — large values stay in the image. Compose by passing prior handles as `inputs`. stdout/stderr are captured and returned. Optional `timeout_ms` (default 10000). [mcp-eval.md](mcp-eval.md) is its contract. |

### Cross-language tracing and invariants

| Tool | Description |
|------|-------------|
| `trace` | Trace an Elle function's calls into the Rust implementation of each primitive it calls, with file and line, and into that function's own Rust callees to a configurable depth. Needs the `syn` plugin. |
| `verify_invariants` | Check project invariants encoded as SPARQL ASK queries (from `.elle-invariants.lisp`). |

## Knowledge graph schema

Three sources fill the graph. At startup the server loads every primitive, and
the Rust sources when the `syn` plugin is present. `analyze_file` adds an Elle
file. The server writes Elle triples through
[lib/rdf/elle.lisp](../lib/rdf/elle.lisp) and Rust triples through
[lib/rdf/rust.lisp](../lib/rdf/rust.lisp); both modules are the schema's
definition. A flag is a string literal, `"true"` or `"false"`, so a query
matches it as `"true"`, never as the boolean `true`.

### Elle function analysis (`urn:elle:Fn`)

| Predicate | Description |
|-----------|-------------|
| `urn:elle:name` | Function name |
| `urn:elle:file` | Source file path |
| `urn:elle:line` | Line of the definition |
| `urn:elle:signal-silent`, `signal-yields`, `signal-io` | The flags `compile/signal` reports |
| `urn:elle:jit-eligible` | `compile/signal`'s `:jit-eligible` |
| `urn:elle:signal-bit` | One signal the function can raise (repeated) |
| `urn:elle:signal-propagates` | The index of a parameter whose signal the function raises (repeated) |
| `urn:elle:calls` | IRI of a function this calls (repeated) |
| `urn:elle:capture` | Name of a captured binding (repeated) |
| `urn:elle:capture-kind` | `value`, `lbox` or `transitive` (repeated; not paired with its capture) |
| `urn:elle:stateless`, `retry-safe`, `parallelizable`, `memoizable`, `timeout-safe` | Composition properties from `std/portrait` |

**Example query: Find all I/O functions**
```sparql
SELECT ?name ?file WHERE {
  ?fn a <urn:elle:Fn> ;
      <urn:elle:name> ?name ;
      <urn:elle:file> ?file ;
      <urn:elle:signal-io> "true" .
}
```

**Example query: Functions that call a specific function**
```sparql
SELECT ?caller ?file WHERE {
  ?caller a <urn:elle:Fn> ;
          <urn:elle:calls> ?target ;
          <urn:elle:file> ?file .
  ?target <urn:elle:name> "map" .
}
```

The graph does not record which capture a function mutates. `impact` and
`compile_parallelize` read that from the analysis itself.

### Other Elle entities

| Type | Predicates |
|------|-----------|
| `urn:elle:Def` | `name`, `file` |
| `urn:elle:Macro` | `name`, `file` |
| `urn:elle:Primitive` | `name`, `category`, `arity`, `doc`, `param`, `alias`, and the signal predicates of a function |
| `urn:elle:Import` | `name`, `path`, `file` — written by the bulk scripts only |

### Rust entities (`urn:rust:` namespace)

| Type | Predicates |
|------|-----------|
| `rust:Fn` | `name`, `file`, `line`, `param`, `param-type`, `return-type`, `async`, `unsafe`, `visibility`, `attribute`, `calls` |
| `rust:Struct` | `name`, `file`, `line`, `kind`, `field`, `field-type`, `visibility`, `attribute` |
| `rust:Enum` | `name`, `file`, `line`, `variant`, `visibility`, `attribute` |
| `rust:Trait`, `rust:Const`, `rust:Static`, `rust:Type`, `rust:Mod` | `name`, `file`, `line`, `visibility`, `attribute` |
| `rust:Use` | `path`, `file`, `visibility` |

A primitive links to its Rust function through `urn:elle:implemented-by`, and
the function back through `urn:rust:implements`. lib/rdf/rust.lisp writes these
links from a `const PRIMITIVES` table in each Rust file. The primitives are now
declared with the `primitive!` macro and no such table exists, so today the
graph holds no link, and `trace` stops at the Elle calls.

## Building and running

The server and its scripts live in the `mcp/` git submodule. A fresh clone
leaves it empty — check it out first:

```bash
git submodule update --init mcp plugins
```

```bash
# Build elle + MCP plugins (oxigraph, syn) in one invocation
make mcp

# Default store location: .elle-mcp/store/ (auto-created)
elle mcp/mcp-server.lisp

# Explicit store path
elle mcp/mcp-server.lisp -- /path/to/store

# Via environment variable
ELLE_MCP_STORE=/path/to/store elle mcp/mcp-server.lisp
```

The store is persistent — graph data survives across server restarts.
The `.elle-mcp/` directory is gitignored.

## Startup behavior

Elle primitive triples are loaded synchronously at startup (fast).
Rust function triples load in a background fiber so the server can
handle requests while the graph loads. This keeps MCP client connection
timeouts from firing on large codebases.

SPARQL queries sent via `sparql_query` have a 30-second timeout. A request
line longer than 10,000,000 characters is rejected with a `-32600` error.

When population completes, the server emits a JSON-RPC notification whose
`rust` field counts the Rust files it loaded:

```json
{"jsonrpc":"2.0","method":"notifications/model/populated","params":{"primitives":true,"rust":406}}
```

Tools that query the graph (`sparql_query`, `trace`, etc.) return
whatever data is available — results may be incomplete until the
notification arrives. Tools that don't depend on the graph
(`initialize`, `ping`, `tools/list`, `analyze_file`, `portrait`, etc.)
work immediately.

The Rust scan reads `src/`, `plugins/*/src/`, `tests/`, `benches/` and
`patches/`, so build artifacts under `target/` are never parsed.

## Populating the graph

The server populates the graph incrementally via `analyze_file`. For
bulk loading, use the supporting scripts. They read each file's forms rather
than analyzing it, so they write names, files, parameters, docstrings and
imports, and no signals:

```bash
# Extract Elle source graph + Rust source graph, load into store
elle mcp/load-all.lisp

# Extract Elle graph only
elle mcp/elle-graph.lisp

# Extract Rust graph only (requires syn plugin)
elle mcp/rust-graph.lisp
```

## What can an AI agent do with it?

**Understand code across language boundaries.** Trace a function from Elle
into Rust:

```text
trace(path: "lib/portrait.lisp", function: "classify-phase", depth: 2)
```

Each Elle call becomes an `[elle]` line. Under a primitive, a `-> [rust]` line
names its Rust function with file and line, followed by that function's Rust
callees. The primitive links described above, under Rust entities, are what
this step reads.

**Assess cascading refactoring impact.** Before changing a function:

```text
impact(path: "lib/http.lisp", function: "parse-url")
```

Returns the function's signal, every function in the file that calls it with
a warning where that caller is silent or JIT-eligible, what it calls, and what
it captures.

**Find functions by behavior.** Which functions do I/O?

```text
signal_query(path: "lib/http.lisp", query: "io")
```

Returns all I/O-performing functions, ready for optimization or scrutiny.

**Refactor safely across the codebase.** Rename a function and all references:

```text
compile_rename(path: "lib/process.lisp", old_name: "helper", new_name: "dispatch")
```

The tool respects lexical scope — shadowed bindings are left alone.

**Query the semantic graph directly.** Any SPARQL query works:

```sparql
# Which functions are JIT-eligible, and how often are they called?
SELECT ?name ?file (COUNT(?caller) as ?calls)
WHERE {
  ?fn a <urn:elle:Fn> ;
      <urn:elle:name> ?name ;
      <urn:elle:file> ?file ;
      <urn:elle:jit-eligible> "true" .
  OPTIONAL {
    ?caller a <urn:elle:Fn> ;
            <urn:elle:calls> ?fn .
  }
}
GROUP BY ?name ?file
ORDER BY DESC(?calls)
```

## Example SPARQL queries for agents

**Find entry points (functions with no callers — dead code or API boundaries)**
```sparql
SELECT ?name ?file
WHERE {
  ?fn a <urn:elle:Fn> ;
      <urn:elle:name> ?name ;
      <urn:elle:file> ?file .
  FILTER NOT EXISTS {
    ?caller a <urn:elle:Fn> ;
            <urn:elle:calls> ?fn .
  }
}
ORDER BY ?file
```

**Find highly connected functions (composition complexity)**
```sparql
SELECT ?name (COUNT(?callee) as ?calls_count)
WHERE {
  ?fn a <urn:elle:Fn> ;
      <urn:elle:name> ?name ;
      <urn:elle:calls> ?callee .
}
GROUP BY ?name
ORDER BY DESC(?calls_count)
LIMIT 20
```

**Find functions that close over a boxed binding (a mutable local, or a
file-level binding)**
```sparql
SELECT DISTINCT ?name ?file
WHERE {
  ?fn a <urn:elle:Fn> ;
      <urn:elle:name> ?name ;
      <urn:elle:file> ?file ;
      <urn:elle:capture-kind> "lbox" .
}
ORDER BY ?file ?name
```

**Check cross-language dependencies (Elle calling Rust primitives)**
```sparql
SELECT ?elle_fn ?rust_fn
WHERE {
  ?elle_fn a <urn:elle:Fn> ;
           <urn:elle:calls> ?prim .
  ?prim <urn:elle:implemented-by> ?rust_fn .
}
```

**Find all functions in a file and their signal profiles**
```sparql
SELECT ?name ?yields ?io ?jit_eligible
WHERE {
  ?fn a <urn:elle:Fn> ;
      <urn:elle:file> "lib/http.lisp" ;
      <urn:elle:name> ?name ;
      <urn:elle:signal-yields> ?yields ;
      <urn:elle:signal-io> ?io ;
      <urn:elle:jit-eligible> ?jit_eligible .
}
ORDER BY ?name
```

See [demo-queries.lisp](https://github.com/elle-lisp/mcp/blob/main/demo-queries.lisp)
in the submodule for more examples.

## Test orchestration

The MCP server provides tools for running tests, recording results, and
gating pushes on test status.

| Tool | Description |
|------|-------------|
| `test_run` | Run tests and record the result, keyed by `(sha, mode)`, in the RDF store. Answers pass or fail, the failure lines, the duration, and whether the worktree was clean. |
| `test_status` | The stored results for a commit, one row per mode. |
| `test_history` | Stored results across recent runs, newest first. |
| `test_gate` | Check if a SHA is clear to push: a passing `test` run recorded for it on a clean worktree. |
| `push_ready` | Push after `test_gate` passes for HEAD; refuse with the reason otherwise. |
| `push_wip` | Push unconditionally, for saving work or requesting review. |

### `test_run`

```json
{"path": "tests/elle/core.lisp", "mode": "single", "jit": "off"}
```

Parameters:
- `mode` (required) — `"smoke"` runs `make smoke`, `"test"` runs `make test`,
  and `"single"` runs one file with `$ELLE`, else `./target/debug/elle`
- `path` — the file, required for `"single"`
- `jit` (optional) — `"off"`, `"eager"` or `"adaptive"`. A single file gets it
  as a command-line flag; a make target gets it as `ELLE_JIT` in the
  environment, which the binary does not read

The answer:
```json
{"passed": false, "failed-count": 1,
 "failures": [{"message": "✗ Runtime error: …"}],
 "duration": …, "clean": true, "sha": "abc123"}
```

A failure is one stderr line that holds `✗`. The stored record keeps the
count and the first 10,000 characters of stderr, not the failure lines.

### `test_status`, `test_gate`, `push_ready` / `push_wip`

`test_status` takes an optional `sha` (default HEAD) and `mode`, and answers
`{"sha": …, "results": [ … ]}` with one stored row per mode. `test_gate` takes
an optional `sha` (default HEAD) and answers `{"ready": true, "sha": …}` or
`{"ready": false, "reason": …}`. `push_ready` and `push_wip` take a required
`branch` and an optional `remote` (default `origin`).

### Test result storage

Results are stored as RDF triples:

```turtle
<urn:test:abc123:smoke> a <urn:elle:TestRun> ;
    <urn:elle:sha> "abc123" ;
    <urn:elle:mode> "smoke" ;
    <urn:elle:clean> true ;
    <urn:elle:passed> true ;
    <urn:elle:duration> … ;
    <urn:elle:timestamp> "…" ;
    <urn:elle:failed-count> 0 ;
    <urn:elle:stderr> "" .
```

## Design rationale

The MCP server exposes what the compiler already computes. Elle's compilation pipeline performs signal inference, capture analysis, and binding resolution for every file. This information exists whether or not anyone queries it — the MCP server just makes it accessible over JSON-RPC.

**Everything the analysis tools report is available to normal Elle code at runtime.** `compile/analyze`, `compile/signal`, `compile/captures`, `compile/callees` — these are regular Elle functions. The MCP server is just an Elle program (`mcp/mcp-server.lisp`) that wraps these primitives in the Model Context Protocol. You can write your own analysis tools using the same functions:

```lisp
(def my-code "(defn my-function [n] (def @total 0)
                (each i in (range n) (assign total (+ total i)))
                (println total))")
(def a (compile/analyze my-code {:file "my-code.lisp"}))

(assert (get (compile/signal a :my-function) :io))            # it prints
(assert (empty? (compile/captures a :my-function)))           # it closes over nothing
(assert (not (empty? (compile/callees a :my-function))))      # it calls range, println, ...
```

See [Design Philosophy](philosophy.md) for why Elle is designed this way, and [Agent Reasoning](analysis/agent-reasoning.md) for how agents use the MCP server in practice.

## The graph is a cache

The knowledge graph is a snapshot of compiler analysis at the time each file was analyzed. It can become stale.

**Source code is ground truth.** If the graph contradicts the source, the source wins.

The server watches the tree and re-analyzes a `.lisp` file when it is created
or written: it drops the cached analysis, clears the file's old triples, and
loads fresh ones. It also sends a `notifications/model/updated` notification
naming the functions whose signal changed. `analyze_file` on a file the server
has already analyzed answers from that cache.

**After refactoring:** a change made via `compile_rename` or `compile_extract`
comes back as new source text. Write it to the file, and the watcher
re-analyzes it; then run the tests.

## IDE integration

The MCP server is designed for AI coding assistants (Claude, Cursor,
Copilot, etc.) that support the Model Context Protocol. Configure your
editor to launch `elle mcp/mcp-server.lisp` as an MCP server.

The server complements the LSP server (`elle lsp`) — LSP handles
real-time editing features (completions, diagnostics, go-to-definition),
while MCP provides deeper structural analysis for AI-driven refactoring
and code understanding.

## Supporting tools

| File | Purpose |
|------|---------|
| [elle-graph.lisp](https://github.com/elle-lisp/mcp/blob/main/elle-graph.lisp) | Extract RDF triples from Elle source files |
| [rust-graph.lisp](https://github.com/elle-lisp/mcp/blob/main/rust-graph.lisp) | Extract RDF triples from Rust source files via syn plugin |
| [lib/rdf/elle.lisp](../lib/rdf/elle.lisp) | Elle→RDF triple generation (`std/rdf/elle`) |
| [lib/rdf/rust.lisp](../lib/rdf/rust.lisp) | Rust→RDF triple generation (`std/rdf/rust`) |
| [load-all.lisp](https://github.com/elle-lisp/mcp/blob/main/load-all.lisp) | Extract both graphs and load into the store |
| [demo-queries.lisp](https://github.com/elle-lisp/mcp/blob/main/demo-queries.lisp) | Example SPARQL queries |
| [test-mcp.lisp](https://github.com/elle-lisp/mcp/blob/main/test-mcp.lisp) | Smoke test: spawns server, exercises the tools |
| [semantic-graph.lisp](https://github.com/elle-lisp/mcp/blob/main/semantic-graph.lisp) | Semantic graph analysis utilities |

## Dependencies

The MCP server requires:
- `oxigraph` plugin — RDF triple store with SPARQL
- `syn` plugin, optional — Rust source parsing, for `trace` and the Rust graph
- the `glob`, `watch` and `uuid` modules — file discovery, the file watcher,
  and eval handles
