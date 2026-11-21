# Code Analysis and Semantic Understanding

<!-- audited: 2026-11-20 -->

The tools that read Elle code without running it: a portrait for one file, and
the MCP graph for a whole codebase.

## The three layers of code understanding

### 1. Local analysis: portraits

[portrait.md](portrait.md) — Analyze a single file without executing it.

Use `compile/analyze` to get:
- Signal profiles (does it yield? do I/O? error?)
- Captures (what does its closure hold?)
- Call graphs (what does it call?)
- Composition properties (parallelizable? memoizable?)
- Observations (non-obvious properties like almost-pure functions or shared mutable state)

**Use case:** Understand a single function or module in depth.

### 2. Global analysis: the MCP knowledge graph

[../mcp.md](../mcp.md) — Query the entire codebase as an RDF knowledge graph.

The MCP server exposes:
- **RDF triples** — Every function, definition, import, and call in the codebase
- **SPARQL interface** — Query the graph for patterns, dependencies, and composition
- **Refactoring tools** — Rename, extract, parallelize (binding-aware and signal-aware)
- **Tracing** — Follow an Elle function's calls

**Use case:** Understand code structure, impact, and composition across files.

### 3. Agent reasoning

[agent-reasoning.md](agent-reasoning.md) — How AI agents reason about Elle code.

Combines portrait (local) + MCP (global) into a workflow:
1. **Understand** — Use portrait to analyze a file
2. **Query** — Use SPARQL to understand codebase-wide impact
3. **Refactor** — Use compile-aware tools for safe changes

**Use case:** Automate code understanding, refactoring, and verification.

## What the compiler knows, and where it shows

The compiler computes signal inference, capture analysis, and call graphs for
every file it compiles. These tools show that information:

| What the compiler knows | How it's exposed |
|---|---|
| Signal profiles (yields, I/O, silent) | Portrait, MCP `elle:signal-*` predicates |
| Call graphs (what calls what) | MCP `elle:calls` triples, SPARQL queries |
| Capture analysis (what's closed over) | Portrait captures, MCP `elle:capture` predicates |
| Binding scope and resolution | `compile_rename`, `compile_extract` |
| Cross-file structure | MCP knowledge graph (populated by `analyze_file`) |

The tools add no information to what the compiler computes. They make it
available outside the compilation pipeline, and [the MCP server](../mcp.md)
says how a program reads it. See [philosophy.md](../philosophy.md) and
[modules.md](../modules.md) for the design rationale.

## For humans

If you're reading Elle code:
- Use [portrait.md](portrait.md) to understand a function's behavior
- Read source code for exact semantics
- Use [../mcp.md](../mcp.md) if you need to understand cross-file impact

## For agents

If you're analyzing Elle code:
- Use `portrait` to understand a file locally
- Use `sparql_query` to find patterns across the codebase
- Use `compile_rename`, `compile_extract`, `compile_parallelize` for safe refactoring
- Use `trace` to follow an Elle function's calls
- See [agent-reasoning.md](agent-reasoning.md) for workflows and patterns
