# Agent Reasoning in Elle

<!-- audited: 2026-09-23 -->

Elle is designed to be easily reasoned about by AI coding assistants. This guide explains how agents should approach code understanding, analysis, and refactoring in Elle.

**Related:** [Portrait](portrait.md) (local file analysis), [MCP Server](../mcp.md) (global knowledge graph), [Analysis README](README.md) (overview of all analysis tools), [Philosophy](../philosophy.md) (design rationale).

## The two-layer approach

Elle separates **human interface** from **machine interface**:

- **Layer 1 (Human)**: The Elle language itself—simple, readable, no formal annotations
- **Layer 2 (Machine)**: Portrait (local analysis) + MCP (global reasoning) give agents complete semantic visibility

An agent doesn't need to understand signals by reading source code. It queries the semantic graph instead.

## Workflow: Understand → Query → Refactor

### Step 1: Understand a function locally (Portrait)

When analyzing a single file or function, use `portrait` to get its profile:

```lisp
# Analyze the file
(def a (compile/analyze (file/read "lib/http.lisp") {:file "lib/http.lisp"}))

# Get a function's portrait, and render it as text
(def portrait-lib ((import "std/portrait")))
(def request-portrait (portrait-lib:function a :http-request))
(assert (string/contains? (portrait-lib:render request-portrait) "Effects:"))

# The same facts, as data
(assert (= (get request-portrait :name) "http-request"))
(assert (get (get request-portrait :signal) :io))              # it does I/O
(assert (not (empty? (get request-portrait :captures))))       # it closes over helpers
```

This reveals:
- **Signal profile**: Does it yield? Do I/O? Error?
- **Composition**: JIT-eligible? Memoizable? Safe to parallelize?
- **Captures**: What variables does it close over?
- **Observations**: Non-obvious properties (almost-pure? Shared mutable state?)

### Step 2: Understand codebase-wide impact (MCP + SPARQL)

When making a change, query the global graph to understand impact. The graph's
vocabulary is [lib/rdf/elle.lisp](../../lib/rdf/elle.lisp): a signal flag or a
composition property is a string literal, `"true"` or `"false"`.

**Before renaming a function:**
```sparql
# Who calls this function?
SELECT ?caller
WHERE {
  ?caller a <urn:elle:Fn> ;
          <urn:elle:calls> ?target .
  ?target <urn:elle:name> "old-name" .
}
```

**Before optimizing a function:**
```sparql
# Is it JIT-eligible? Do its callers depend on its signal?
SELECT ?name ?jit ?file
WHERE {
  ?fn a <urn:elle:Fn> ;
      <urn:elle:name> "my-function" ;
      <urn:elle:jit-eligible> ?jit ;
      <urn:elle:file> ?file .
}
```

**To find optimization opportunities:**
```sparql
# Which functions are called most frequently? (composition hotspots)
SELECT ?name (COUNT(?caller) as ?inbound)
WHERE {
  ?caller a <urn:elle:Fn> ;
          <urn:elle:calls> ?fn .
  ?fn <urn:elle:name> ?name .
}
GROUP BY ?name
ORDER BY DESC(?inbound)
LIMIT 10
```

### Step 3: Refactor safely using compile-aware tools

Don't edit text directly. Use the compile-safe tools:

**Rename a function and all references:**
```text
compile_rename(path: "lib/http.lisp", old_name: "request-handler", new_name: "handle-request")
```
This respects lexical scope—shadowed bindings are left alone.

**Extract a code region into its own function:**
```text
compile_extract(path: "lib/http.lisp", from: "process-request", start_line: 10, end_line: 25, name: "validate-headers")
```
The tool computes free variables (which become parameters) and infers the extracted function's signal.

**Check if functions can run in parallel:**
```text
compile_parallelize(path: "lib/process.lisp", functions: ["worker1", "worker2", "worker3"])
```
This verifies no shared mutable captures would cause data races.

The three tools are the MCP faces of `compile/rename`, `compile/extract` and
`compile/parallelize`, which a program can call on an analysis directly.

### Step 4: Verify

After any refactoring, run the tests. The server watches the tree for `.lisp`
files and re-analyzes a file when it is written, so the graph follows the
edit, and `analyze_file` reports that analysis:

```text
analyze_file(path: "lib/http.lisp")
```

The knowledge graph is a cache of compiler analysis. Source code is always ground truth. If the graph seems wrong after the file has been re-analyzed, it's a compiler bug — not a code error.

## Signal reasoning for agents

Agents can reason about signals without reading code—just query the graph.

### Signal propagation

If function A calls function B, A's signal is at least as broad as B's. Query this relationship:

```sparql
# A silent function that calls a yielding one
SELECT ?fn1 ?fn2
WHERE {
  ?fn1 a <urn:elle:Fn> ;
       <urn:elle:calls> ?fn2 ;
       <urn:elle:signal-yields> "false" .
  ?fn2 <urn:elle:signal-yields> "true" .
}
```

Inference already carries a callee's signal into its caller, so this query
answers nothing on a current graph. A row means the graph is stale or the
compiler is wrong.

### Finding constraints

A function that calls one of its parameters takes its signal from that
argument. The graph records the parameter's position:

```sparql
# Functions whose signal comes from a callback parameter
SELECT ?name ?param
WHERE {
  ?fn a <urn:elle:Fn> ;
      <urn:elle:name> ?name ;
      <urn:elle:signal-propagates> ?param .
}
```

Then use the `impact` tool to see downstream implications of changing those callbacks.

## Cross-language understanding

Use the `trace` tool to follow an Elle function's calls into their Rust
implementations. It needs the `syn` plugin, which builds the Rust half of the
graph:

```text
trace(path: "lib/portrait.lisp", function: "classify-phase", depth: 2)
```

For each call the function makes, it prints an `[elle]` line with the callee's
line, tail position and IRI. Under a primitive it prints `-> [rust]` with the
Rust function that implements it, its file and line, and that function's own
Rust callees to the requested depth. Under an Elle-defined callee it prints the
callee's signal instead.

The step from a primitive to its Rust function reads
`<urn:elle:implemented-by>` links. [lib/rdf/rust.lisp](../../lib/rdf/rust.lisp)
writes them from a `const PRIMITIVES` table in each Rust file. The primitives
are now declared with the `primitive!` macro and no such table exists, so today
no primitive carries the link and a trace stops at its `[elle]` lines.

### What agents can do with traces

Where the links exist, a trace answers these questions:

**Find performance bottlenecks:**
1. Trace a hot function
2. See which Rust implementations it calls
3. Check if those Rust functions have complex logic (read the source)
4. Look for data structure traversals, allocations, or system calls

**Understand cost of operations:**
Each Elle operation maps to one or more Rust functions, and the trace names them.

**Identify optimization opportunities:**
- Are certain primitives called repeatedly? Consider caching
- Does a function call through many layers? Consider a specialized primitive
- Is there redundant work across calls? Fusion/optimization opportunity

**Cross-language debugging:**
If behavior is unexpected, trace shows exactly which Rust code is involved.
An agent can then read the Rust source to understand semantics.

## Invariant checking

Write project-level invariants as SPARQL ASK queries in `.elle-invariants.lisp`.
The file holds one Elle collection of structs, each naming a query and the
answer it must give. `verify_invariants` reads the file, evaluates it, runs
each query, and compares the answer with `:expect`:

```lisp
(def invariants-source
  "[{:name \"no silent function calls a yielding one\"
     :query \"ASK { ?f <urn:elle:signal-yields> 'false' ;
                       <urn:elle:calls> ?g .
                    ?g <urn:elle:signal-yields> 'true' . }\"
     :expect false}]")

# What the server does with the file's text
(def invariants (eval (read invariants-source)))
(assert (= (get (first invariants) :expect) false))
(assert (string/starts-with? (get (first invariants) :query) "ASK"))
```

Then verify via:
```text
verify_invariants()
```

Agents can use this to ensure code meets invariants before committing changes.

## Why this approach works for agents

1. **No ambiguity** — Signals are explicit in the RDF, not inferred from reading code
2. **Queryable** — SPARQL is expressive enough to find any structural pattern
3. **Composable** — Small, focused tools (rename, extract, parallelize) that combine
4. **Language-agnostic** — Agents don't need to parse Elle syntax; they query the graph
5. **Safe** — Refactoring tools understand binding and scoping rules

An agent doesn't need to understand Elle's syntax deeply. It understands the **semantic model** the tools expose.

## Common agent patterns

### Pattern 1: Optimize a hot path

1. Query: "Which functions are called most?"
2. Query: "Which are not JIT-eligible?"
3. Use `impact` to check: "What would change if I made this silent?"
4. Use `compile_extract` to factor out the I/O or yielding parts
5. Verify: "Check invariants"

### Pattern 2: Detect data races

1. Query: "Which variables are captured by multiple functions?"
2. Query: "Are any of those captures mutated?"
3. Use `portrait` to understand each function's role
4. Propose: synchronization or refactoring to remove shared state
5. Use `compile_parallelize` to verify the fix

### Pattern 3: Understand cascading changes

1. Query: "Who calls this function?"
2. For each caller: Query: "What are their callers?"
3. Build the impact graph
4. Use `compile_rename` for safe mass-refactoring
5. Verify: "Check invariants"

### Pattern 4: Find abstraction boundaries

1. Query: "Which functions have no outbound calls?" (pure/data-processing)
2. Query: "Which functions have no inbound calls?" (entry points)
3. Use `portrait` without a function name to read the module's profile

## See also

- [portrait.md](portrait.md) — Local function analysis
- [mcp.md](../mcp.md) — Global semantic graph and query interface
- [signals/index.md](../signals/index.md) — Signal system design
- [modules.md](../modules.md) — Module system and composition
