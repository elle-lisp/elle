# MCP Server

Elle ships with an [MCP](https://modelcontextprotocol.io) (Model Context
Protocol) server that gives AI coding assistants deep, structured access
to an Elle codebase. The server is itself written in Elle
([`tools/mcp-server.lisp`](../tools/mcp-server.lisp)) and communicates via JSON-RPC 2.0 on stdio.

**See also:** [Agent Reasoning in Elle](analysis/agent-reasoning.md) for how to use MCP + portrait together. [Portrait](analysis/portrait.md) for local file analysis. [Analysis directory](analysis/) for an overview of code understanding tools.

## What it does

The server maintains a persistent RDF knowledge graph (via the oxigraph
plugin) that represents the structure of both Elle and Rust source code.
It exposes 15 tools that let an AI agent query, analyze, and refactor
code through the graph rather than through ad-hoc text searches.

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
| `analyze_file` | Analyze an Elle source file — extracts symbols, signals, diagnostics, and observations. Populates the RDF graph with function definitions, arities, parameters, and docstrings. |
| `portrait` | Semantic portrait of a function or module. Shows the effect profile (silent/yields/errors), failure modes, composition properties, and human-readable observations. Omit the function name for a module-level portrait. |
| `signal_query` | Find functions matching a signal property: `silent`, `io`, `yields`, `jit-eligible`, `errors`, or any registered signal keyword. |
| `impact` | Assess the impact of changing a function. Shows callers, downstream signal implications, and JIT eligibility changes. |

### Refactoring

| Tool | Description |
|------|-------------|
| `compile_rename` | Binding-aware rename of a function or variable and all its references. Understands lexical scope — won't rename shadowed bindings. |
| `compile_extract` | Extract a line range into a new function. Computes free variables (which become parameters) and infers the extracted function's signal. |
| `compile_parallelize` | Check if a set of functions can safely run in parallel. Verifies no shared mutable captures. |

### Cross-language tracing

| Tool | Description |
|------|-------------|
| `trace` | Trace an Elle function through primitives into the Rust implementation. For each Elle function call, shows exactly which Rust primitive it maps to (with file/line), and what Rust functions that primitive calls. Complete end-to-end call chain with source locations. Configurable depth. |
| `verify_invariants` | Check project invariants encoded as SPARQL ASK queries (from `.elle-invariants.lisp`). |

## Knowledge graph schema

The graph is populated by `analyze_file` (Elle sources) and the
supporting [`tools/elle-graph.lisp`](../tools/elle-graph.lisp) and [`tools/rust-graph.lisp`](../tools/rust-graph.lisp) scripts.

### Elle function analysis (`urn:elle:Fn`)

Each function is represented with complete signal and composition metadata:

| Predicate | Type | Description |
|-----------|------|-------------|
| `elle:name` | string | Function name |
| `elle:file` | string | Source file path |
| `elle:arity` | integer | Number of parameters |
| `elle:param` | string | Parameter name (repeated) |
| `elle:doc` | string | Docstring if present |
| `elle:signal-yields` | boolean | True if function may yield |
| `elle:signal-io` | boolean | True if function does I/O |
| `elle:signal-error` | boolean | True if function may error |
| `elle:jit-eligible` | boolean | True if JIT-compilable (silent, no I/O) |
| `elle:calls` | IRI | Function this calls (repeated) |
| `elle:capture` | string | Variable this captures (repeated) |
| `elle:capture-mutated` | string | Captured variable that is mutated (repeated) |

**Example query: Find all I/O functions**
```sparql
SELECT ?name ?file WHERE {
  ?fn a <urn:elle:Fn> ;
      <urn:elle:name> ?name ;
      <urn:elle:file> ?file ;
      <urn:elle:signal-io> true .
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

**Example query: Potentially shared mutable state (race condition risk)**
```sparql
SELECT ?var (COUNT(?fn) as ?captures)
WHERE {
  ?fn a <urn:elle:Fn> ;
      <urn:elle:capture-mutated> ?var .
}
GROUP BY ?var
HAVING (?captures > 1)
```

### Other Elle entities

| Type | Predicates |
|------|-----------|
| `elle:Def` | `elle:name`, `elle:file` |
| `elle:Macro` | `elle:name`, `elle:file` |
| `elle:Import` | `elle:name`, `elle:path`, `elle:file` |
| `elle:Primitive` | `elle:name`, `elle:arity`, `elle:doc`, `elle:signal-*` (same as Fn) |

### Rust entities (`urn:rust:` namespace)

| Type | Predicates |
|------|-----------|
| `rust:Fn` | `rust:name`, `rust:file`, `rust:param`, `rust:param-type`, `rust:return-type`, `rust:async`, `rust:unsafe`, `rust:visibility`, `rust:attribute` |
| `rust:Struct` | `rust:name`, `rust:file`, `rust:kind`, `rust:field`, `rust:field-type`, `rust:visibility`, `rust:attribute` |
| `rust:Enum` | `rust:name`, `rust:file`, `rust:variant`, `rust:visibility`, `rust:attribute` |
| `rust:Trait` | `rust:name`, `rust:file`, `rust:visibility`, `rust:attribute` |
| `rust:Const` | `rust:name`, `rust:file`, `rust:visibility`, `rust:attribute` |
| `rust:Static` | `rust:name`, `rust:file`, `rust:visibility`, `rust:attribute` |
| `rust:Type` | `rust:name`, `rust:file`, `rust:visibility`, `rust:attribute` |
| `rust:Mod` | `rust:name`, `rust:file`, `rust:visibility`, `rust:attribute` |

## Running the server

```bash
# Default store location: .elle-mcp/store/ (auto-created)
elle tools/mcp-server.lisp

# Explicit store path
elle tools/mcp-server.lisp -- /path/to/store

# Via environment variable
ELLE_MCP_STORE=/path/to/store elle tools/mcp-server.lisp
```

The store is persistent — graph data survives across server restarts.
The `.elle-mcp/` directory is gitignored by default.

## Populating the graph

The server populates the graph incrementally via `analyze_file`. For
bulk loading, use the supporting scripts:

```bash
# Extract Elle source graph + Rust source graph, load into store
elle tools/load-all.lisp

# Extract Elle graph only
elle tools/elle-graph.lisp

# Extract Rust graph only (requires syn plugin)
elle tools/rust-graph.lisp
```

## What can an AI agent do with it?

**Understand code across language boundaries.** Trace a function from Elle through Rust implementations:

```
trace(path: "lib/portrait.lisp", function: "classify-phase", depth: 2)
```

Returns the complete call chain with source locations:

```
[elle] get (line 31, tail=false)
  -> [rust] prim_get src/primitives/access.rs:44
       [rust] resolve_index src/primitives/access.rs:11
       [rust] error_val src/value/error.rs:10

[elle] empty? (line 32, tail=false)
  -> [rust] prim_empty src/primitives/list/mod.rs:590
```

Every Elle operation maps to Rust implementations with exact file/line information. Agents can:
- Find performance bottlenecks by seeing which primitives are called
- Understand the cost of operations (e.g., every struct access goes through error handling)
- Read Rust source code for specific operations
- Identify optimization opportunities (repeated patterns, unnecessary layers, etc.)

**Assess cascading refactoring impact.** Before changing a primitive:

```
impact(path: "src/primitives/list/mod.rs", function: "prim_first")
```

Returns every Elle function that calls `first`, their signals, whether they're JIT-compiled, and what would change if you modified `prim_first`.

**Find functions by behavior.** Which functions do I/O?

```
signal_query(path: "lib/http.lisp", query: "io")
```

Returns all I/O-performing functions, ready for optimization or scrutiny.

**Refactor safely across the codebase.** Rename a function and all references:

```
compile_rename(path: "lib/process.lisp", old_name: "helper", new_name: "dispatch")
```

The tool respects lexical scope — shadowed bindings are left alone.

**Query the semantic graph directly.** Any SPARQL query works:

```sparql
# Which functions are JIT-eligible? (performance candidates)
SELECT ?name ?file (COUNT(?caller) as ?calls)
WHERE {
  ?fn a <urn:elle:Fn> ;
      <urn:elle:name> ?name ;
      <urn:elle:file> ?file ;
      <urn:elle:jit-eligible> true .
  OPTIONAL {
    ?caller a <urn:elle:Fn> ;
            <urn:elle:calls> ?fn .
  }
}
GROUP BY ?name ?file
ORDER BY DESC(?calls)
```

## Example SPARQL queries for agents

**Find all JIT-eligible functions (performance hotspots to optimize)**
```sparql
SELECT ?name ?file WHERE {
  ?fn a <urn:elle:Fn> ;
      <urn:elle:name> ?name ;
      <urn:elle:file> ?file ;
      <urn:elle:jit-eligible> true .
}
```

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

**Find functions that capture mutable state (potential correctness issues)**
```sparql
SELECT ?name ?var
WHERE {
  ?fn a <urn:elle:Fn> ;
      <urn:elle:name> ?name ;
      <urn:elle:capture-mutated> ?var .
}
ORDER BY ?var ?name
```

**Check cross-language dependencies (Elle calling Rust primitives)**
```sparql
SELECT ?elle_fn ?rust_fn
WHERE {
  ?elle_fn a <urn:elle:Fn> ;
           <urn:elle:calls> ?calls_iri .
  ?prim a <urn:elle:Primitive> .
  # Match by name conversion (elle-style to rust style)
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

See [`tools/demo-queries.lisp`](../tools/demo-queries.lisp) for more examples.

## Design rationale

The MCP server exposes what the compiler already computes. Elle's compilation pipeline performs signal inference, capture analysis, and binding resolution for every file. This information exists whether or not anyone queries it — the MCP server just makes it accessible over JSON-RPC.

**Everything the MCP server provides is available to normal Elle code at runtime.** `compile/analyze`, `compile/signal`, `compile/captures`, `compile/callees` — these are regular Elle functions. The MCP server is just an Elle program (`tools/mcp-server.lisp`) that wraps these primitives in the Model Context Protocol. You can write your own analysis tools using the same functions:

```lisp
(def a (compile/analyze (file/read "my-code.lisp") {:file "my-code.lisp"}))
(compile/signal a :my-function)    # => signal profile
(compile/captures a :my-function)  # => captured variables
(compile/callees a :my-function)   # => call graph
```

See [Design Philosophy](philosophy.md) for why Elle is designed this way, and [Agent Reasoning](analysis/agent-reasoning.md) for how agents use the MCP server in practice.

## The graph is a cache

The knowledge graph is a snapshot of compiler analysis at the time each file was analyzed. It can become stale.

**Source code is ground truth.** If the graph contradicts the source, the source wins. Re-analyze the file:

```text
analyze_file(path: "lib/http.lisp")
```

**When to re-analyze:**
- After editing a file
- When `portrait` or `signal_query` results seem wrong
- Before trusting impact analysis for a refactoring decision

**After refactoring:** Any change made via `compile_rename`, `compile_extract`, or manual editing should be followed by re-analysis of the affected files and a test run. The refactoring tools produce correct transformations, but the graph won't reflect the new state until those files are re-analyzed.

The MCP server's `analyze_file` tool handles this — it clears old triples for the file and replaces them with fresh analysis.

## IDE integration

The MCP server is designed for AI coding assistants (Claude, Cursor,
Copilot, etc.) that support the Model Context Protocol. Configure your
editor to launch `elle tools/mcp-server.lisp` as an MCP server.

The server complements the LSP server (`elle lsp`) — LSP handles
real-time editing features (completions, diagnostics, go-to-definition),
while MCP provides deeper structural analysis for AI-driven refactoring
and code understanding.

## Supporting tools

| File | Purpose |
|------|---------|
| [`tools/elle-graph.lisp`](../tools/elle-graph.lisp) | Extract RDF triples from Elle source files |
| [`tools/rust-graph.lisp`](../tools/rust-graph.lisp) | Extract RDF triples from Rust source files via syn plugin |
| [`tools/rust-rdf-lib.lisp`](../tools/rust-rdf-lib.lisp) | Shared library for Rust→RDF extraction |
| [`tools/load-all.lisp`](../tools/load-all.lisp) | Extract both graphs and load into the store |
| [`tools/demo-queries.lisp`](../tools/demo-queries.lisp) | Example SPARQL queries |
| [`tools/test-mcp.lisp`](../tools/test-mcp.lisp) | Smoke test: spawns server, exercises all tools |
| [`tools/semantic-graph.lisp`](../tools/semantic-graph.lisp) | Semantic graph analysis utilities |

## Dependencies

The MCP server requires:
- `oxigraph` plugin — RDF triple store with SPARQL
- `syn` plugin — Rust source parsing (for `trace` and Rust graph extraction)
- `glob` module — file discovery (for `load-all.lisp`)
