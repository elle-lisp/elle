# Code Analysis and Semantic Understanding

This directory contains documentation for understanding and reasoning about Elle code—both locally (within a file) and globally (across the entire codebase).

## The Three Layers of Code Understanding

### 1. Local Analysis: `portrait.lisp`

[portrait.md](portrait.md) — Analyze a single file without executing it.

Use `compile/analyze` to get:
- Signal profiles (does it yield? do I/O? error?)
- Capture analysis (what variables does it close over?)
- Call graphs (what does it call?)
- Composition properties (JIT-eligible? parallelizable? memoizable?)
- Observations (non-obvious properties like almost-pure functions or shared mutable state)

**Use case:** Understand a single function or module in depth.

### 2. Global Analysis: MCP Knowledge Graph

[../mcp.md](../mcp.md) — Query the entire codebase as an RDF knowledge graph.

The MCP server exposes:
- **RDF triples** — Every function, definition, import, and call in the codebase
- **SPARQL interface** — Query the graph for patterns, dependencies, and composition
- **Refactoring tools** — Rename, extract, parallelize (binding-aware and signal-aware)
- **Tracing** — Follow Elle functions through Rust primitives into implementation

**Use case:** Understand code structure, impact, and composition across files.

### 3. Agent Reasoning

[agent-reasoning.md](agent-reasoning.md) — How AI agents reason about Elle code.

Combines portrait (local) + MCP (global) into a workflow:
1. **Understand** — Use portrait to analyze a file
2. **Query** — Use SPARQL to understand codebase-wide impact
3. **Refactor** — Use compile-aware tools for safe changes

**Use case:** Automate code understanding, refactoring, and verification.

## Relationship to Language Design

The compiler already computes signal inference, capture analysis, and call graphs for every file it compiles. These tools surface that information:

| What the compiler knows | How it's exposed |
|---|---|
| Signal profiles (yields, I/O, silent) | Portrait, MCP `elle:signal-*` predicates |
| Call graphs (what calls what) | MCP `elle:calls` triples, SPARQL queries |
| Capture analysis (what's closed over) | Portrait captures, MCP `elle:capture` predicates |
| Binding scope and resolution | `compile_rename`, `compile_extract` |
| Cross-file structure | MCP knowledge graph (populated by `analyze_file`) |

The language design (polymorphic-by-default, dynamic modules, per-file compilation) is sound. These tools don't compensate for gaps — they expose what the compiler already computes so that humans and agents can query it.

See [philosophy.md](../../philosophy.md) and [modules.md](../modules.md) for the design rationale.

## For Humans

If you're reading Elle code:
- Use [portrait.md](portrait.md) to understand a function's behavior
- Read source code for exact semantics
- Use [../mcp.md](../mcp.md) if you need to understand cross-file impact

## For Agents

If you're analyzing Elle code:
- Use `portrait` to understand a file locally
- Use `sparql_query` to find patterns across the codebase
- Use `compile_rename`, `compile_extract`, `compile_parallelize` for safe refactoring
- Use `trace` to understand cross-language behavior (Elle → Rust)
- See [agent-reasoning.md](agent-reasoning.md) for workflows and patterns

## Design Principle

The compiler already knows everything these tools expose. Portrait and MCP don't add information — they make existing compiler analysis accessible outside the compilation pipeline.

Everything is available to normal Elle code at runtime via `compile/analyze`, `compile/signal`, `compile/captures`, `compile/callees`, and related functions. The MCP server is itself just an Elle program that wraps these primitives in JSON-RPC. You can build your own analysis tools using the same functions.
