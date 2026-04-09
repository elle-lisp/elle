# Design Philosophy

This document explains the reasoning behind Elle's core architectural decisions.

## Design Goal: Frictionless Async, Explicit Performance

Elle's default is **polymorphic** — functions may yield, spawn fibers, or do I/O unless explicitly marked `(silence)`. This is not a shortcoming; it's a deliberate choice reflecting Elle's primary use case: building concurrent systems with fibers, signals, and async I/O.

### Why Polymorphic-by-Default?

The alternative—defaulting to `(silence)` and requiring explicit opt-in for async code—would be aggressively friction-full:

```text
# Silent by default: every higher-order function requires explicit opt-in
(defn filter [predicate items]
  (allow-yield predicate)                    # <- required boilerplate
  (each items (fn [x] (allow-yield x) ...))) # <- required everywhere

# Every callback, spawn, I/O operation: explicitly allowed to yield
(ev/spawn (allow-yield (fn [] ...)))
(map/call (allow-yield transform) data)
```

In Elle's actual use case—concurrent systems, fiber-based concurrency, signal-driven I/O—this would mean marking 90% of user code as explicitly allowing async. Frictionless development is more important than compile-time performance defaults for the systems Elle is designed to build.

Polymorphic-by-default keeps the path of least resistance aligned with Elle's semantics: "this is async code that may yield."

### Shifting Performance to the 10% Case

The minority case—tight numerical loops, performance-critical inner functions—explicitly uses `(silence)`:

```lisp
(defn add [x y]
  (silence)
  (+ x y))

(add 1 2)
```

This is similar to how Rust puts `unsafe` on unsafe code, or how Python puts `@jit` on hot code. The burden of intent is acceptable when applied to the minority case, not the majority.

## The Semantic Gap: Visibility, Not Design

The challenge is not that polymorphic-by-default is wrong—it's that **signal implications are invisible in source code**. While the signal system is mathematically elegant, it creates a gap between the developer's expectations and the runtime behavior.

### 1. The Hidden Performance Cliff
Because the default is polymorphic, a developer may write code that appears synchronous and tight but silently yields. A single data-dependent branch triggering `SIG_YIELD` can transform an $O(N)$ loop into expensive context switches, without visual indication.

### 2. The Visibility Problem
There is no syntax highlighting, gutter icon, or visual marker showing that a function yields. The polymorphic signal is invisible until you learn it at runtime through performance testing.

### 3. The Hardening Friction
When performance becomes critical, converting a flexible system to hardened-with-silence is labor-intensive. You discover late which code actually needs to be silent. This is analogous to retrofitting type annotations in Python or satisfying the borrow checker in Rust—it's the cost of changing constraints after the fact.

---

## Reasoning About Signals

A function is polymorphic unless marked `(silence)`. When you call a function, consider:

- Does it take higher-order functions (callbacks, predicates)? Then it's polymorphic unless those parameters are bounded by `(silence)`.
- Does it use I/O (ports, subprocesses)? Then it yields.
- Does it use fibers (`ev/spawn`, `ev/join`)? Then it yields.
- Does it call other functions? It's at least as broad as they are.

Mark a function `(silence)` when:

1. **Performance is critical** — tight loops, hot paths, algorithms where yield overhead matters
2. **You're confident it won't yield** — you've read the callees, you know they're silent
3. **The contract is important** — you want to guarantee to callers that this function won't suspend

Don't mark silence on everything. It's an explicit performance contract, not a default.

The compiler already knows all of this — signal inference is computed at compile time. The [portrait](docs/analysis/portrait.md) system exposes it as queryable data, and the [MCP server](docs/mcp.md) makes the entire codebase's signal structure available as an RDF knowledge graph. These tools don't fix a broken design; they surface what the compiler already computes. See [Agent Reasoning in Elle](docs/analysis/agent-reasoning.md) for how AI agents use this.

---

## See also

- [Module system](docs/modules.md) — Architectural constraints of the module system and their rationale
- [Signal inference](docs/signals/inference.md) — How signals are inferred, bounded, and enforced
- [Agent Reasoning](docs/analysis/agent-reasoning.md) — How AI agents analyze and refactor Elle code
- [MCP Server](docs/mcp.md) — Semantic knowledge graph and querying interface
