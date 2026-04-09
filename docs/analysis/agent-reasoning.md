# Agent Reasoning in Elle

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

# Get a function's portrait
(def portrait-lib ((import "std/portrait")))
(println (portrait-lib:render (portrait-lib:function a :make-request)))
```

This reveals:
- **Signal profile**: Does it yield? Do I/O? Error?
- **Composition**: JIT-eligible? Memoizable? Safe to parallelize?
- **Captures**: What variables does it close over?
- **Observations**: Non-obvious properties (almost-pure? Shared mutable state?)

### Step 2: Understand codebase-wide impact (MCP + SPARQL)

When making a change, query the global graph to understand impact:

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
```
compile_rename(path: "lib/http.lisp", old_name: "request-handler", new_name: "handle-request")
```
This respects lexical scope—shadowed bindings are left alone.

**Extract a code region into its own function:**
```
compile_extract(path: "lib/http.lisp", from: "process-request", start_line: 10, end_line: 25, name: "validate-headers")
```
The tool computes free variables (which become parameters) and infers the extracted function's signal.

**Check if functions can run in parallel:**
```
compile_parallelize(path: "lib/process.lisp", functions: ["worker1", "worker2", "worker3"])
```
This verifies no shared mutable captures would cause data races.

## Signal reasoning for agents

Agents can reason about signals without reading code—just query the graph.

### Signal propagation

If function A calls function B, A's signal is at least as broad as B's. Query this relationship:

```sparql
# Get the transitive closure of signal implications
SELECT ?fn1 ?fn2
WHERE {
  ?fn1 a <urn:elle:Fn> ;
       <urn:elle:calls> ?fn2 .
  ?fn1 <urn:elle:signal-yields> ?yields1 .
  ?fn2 <urn:elle:signal-yields> ?yields2 .
  FILTER (NOT ?yields1 && ?yields2)  # fn1 is silent but calls yielding fn2
}
```

This finds violations—functions that claim to be silent but call yielding functions.

### Finding constraints

Parameter bounds create signal constraints. Query functions that take constrained callbacks:

```sparql
# Functions that take callbacks (potential for delegation)
SELECT ?name ?captures
WHERE {
  ?fn a <urn:elle:Fn> ;
      <urn:elle:name> ?name ;
      <urn:elle:capture> ?captures .
  FILTER (CONTAINS(STR(?captures), "f") || CONTAINS(STR(?captures), "callback"))
}
```

Then use the `impact` tool to see downstream implications of changing those callbacks.

## Cross-language understanding

Use the `trace` tool to understand exactly how Elle functions map to Rust implementations:

```
trace(path: "lib/portrait.lisp", function: "classify-phase", depth: 2)
```

Returns a complete call chain showing Elle calls → Rust primitives → Rust implementation:

```
[elle] get (line 31, tail=false)
  -> [rust] prim_get src/primitives/access.rs:44
       [rust] resolve_index src/primitives/access.rs:11
       [rust] get src/config.rs:27
       [rust] error_val src/value/error.rs:10

[elle] empty? (line 32, tail=false)
  -> [rust] prim_empty src/primitives/list/mod.rs:590
       [rust] error_val src/value/error.rs:10
```

Each line shows:
- **Language** — Elle or Rust
- **Function name** — what's being called
- **Source location** — exact file and line number
- **IRIs** — RDF identifiers for the graph

### What agents can do with traces

**Find performance bottlenecks:**
1. Trace a hot function
2. See which Rust implementations it calls
3. Check if those Rust functions have complex logic (read the source)
4. Look for data structure traversals, allocations, or system calls

**Understand cost of operations:**
Each Elle operation maps to one or more Rust functions. Trace shows the cost:
- `(get struct :key)` → `prim_get` → `resolve_index` + `get` + error handling
- The trace reveals that every struct access has error handling overhead

**Identify optimization opportunities:**
- Are certain primitives called repeatedly? Consider caching
- Does a function call through many layers? Consider a specialized primitive
- Is there redundant work across calls? Fusion/optimization opportunity

**Cross-language debugging:**
If behavior is unexpected, trace shows exactly which Rust code is involved.
An agent can then read the Rust source to understand semantics.

## Invariant checking

Define project-level invariants as SPARQL ASK queries in `.elle-invariants.lisp`:

```lisp
# No mutable state shared across functions
(defn check-shared-mutable []
  '(ASK WHERE {
     ?var a <urn:elle:Var> ;
          <urn:elle:captured-by> ?f1 ;
          <urn:elle:captured-by> ?f2 .
     FILTER (?f1 != ?f2)
   }))
```

Then verify via:
```
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
2. Query: "Which are not JIT-eligible and why?"
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
3. Use `compile_extract` to consolidate related functions into modules
4. Use `portrait:module` to understand the new module's profile

## See also

- [portrait.md](portrait.md) — Local function analysis
- [mcp.md](../mcp.md) — Global semantic graph and query interface
- [signals/index.md](../signals/index.md) — Signal system design
- [modules.md](../modules.md) — Module system and composition
