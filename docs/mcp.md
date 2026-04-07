# MCP Server

Elle ships with an [MCP](https://modelcontextprotocol.io) (Model Context
Protocol) server that gives AI coding assistants deep, structured access
to an Elle codebase. The server is itself written in Elle
([`tools/mcp-server.lisp`](../tools/mcp-server.lisp)) and communicates via JSON-RPC 2.0 on stdio.

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
| `trace` | Trace an Elle function through primitives into the Rust implementation. Shows the full call chain: Elle code → Elle primitives → Rust functions → deeper Rust calls. Configurable depth. |
| `verify_invariants` | Check project invariants encoded as SPARQL ASK queries (from `.elle-invariants.lisp`). |

## Knowledge graph schema

The graph is populated by `analyze_file` (Elle sources) and the
supporting [`tools/elle-graph.lisp`](../tools/elle-graph.lisp) and [`tools/rust-graph.lisp`](../tools/rust-graph.lisp) scripts.

### Elle entities (`urn:elle:` namespace)

| Type | Predicates |
|------|-----------|
| `elle:Fn` | `elle:name`, `elle:file`, `elle:arity`, `elle:param`, `elle:doc` |
| `elle:Def` | `elle:name`, `elle:file` |
| `elle:Macro` | `elle:name`, `elle:file` |
| `elle:Import` | `elle:name`, `elle:path`, `elle:file` |

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

**Understand code across language boundaries.** Ask the `trace` tool to
follow `map` from Elle into Rust:

```
trace(path: "stdlib.lisp", function: "map")
```

Returns the full call chain: `map` (Elle) → calls `cons`, `first`,
`rest` (primitives) → Rust `prim_cons`, `prim_first`, `prim_rest` →
`Value::cons()`, `Cons::first`, `Cons::rest`.

**Assess refactoring impact.** Before changing `prim_first`:

```
impact(path: "src/primitives/list/mod.rs", function: "prim_first")
```

Returns every Elle function that calls `first`, what their signals are,
and whether any are JIT-compiled.

**Find functions by behavior.** Which functions do I/O?

```
signal_query(path: "lib/http.lisp", query: "io")
```

**Refactor safely.** Rename a function and all its references:

```
compile_rename(path: "lib/process.lisp", old_name: "helper", new_name: "dispatch")
```

The rename respects lexical scope — shadowed bindings are left alone.

**Query the graph directly.** Any SPARQL query works:

```sparql
# Which files import the most modules?
SELECT ?file (COUNT(*) AS ?imports) WHERE {
  ?s a <urn:elle:Import> ; <urn:elle:file> ?file
} GROUP BY ?file ORDER BY DESC(?imports)
```

## Example SPARQL queries

Find all functions that can error:

```sparql
SELECT ?name ?file WHERE {
  ?fn a <urn:elle:Fn> ;
      <urn:elle:name> ?name ;
      <urn:elle:file> ?file .
  # Filter by signal if the graph includes signal triples
}
```

Find all Rust structs:

```sparql
SELECT ?name ?file WHERE {
  ?s a <urn:rust:Struct> ;
     <urn:rust:name> ?name ;
     <urn:rust:file> ?file .
}
```

See [`tools/demo-queries.lisp`](../tools/demo-queries.lisp) for more examples.

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
