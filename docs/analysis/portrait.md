# Portrait

The portrait system exposes everything the compiler knows about your code:
signal profiles, capture analysis, composition properties, and the call graph.
It analyzes source without executing it.

**See also:** [Agent Reasoning in Elle](agent-reasoning.md) for how to use portrait + MCP together for codebase-wide analysis. For global codebase queries, see [MCP server](../mcp.md).

## Compile-time analysis

```text
(def src "(defn validate [data]
  (when (nil? (get data :name))
    (error {:error :validation-error :message \"missing name\"}))
  data)")

(def a (compile/analyze src {:file "example.lisp"}))
```

## Signal queries

```text
# Query a function's inferred signal profile
(compile/signal a :validate)
# => {:silent false :jit-eligible true :propagates ... }

# Query what a closure captures
(compile/captures a :process)
# => (:count :config)

# Query what a function calls
(compile/callees a :process)
# => (:validate :transform ...)

# Full call graph
(compile/call-graph a)
```

## Portrait library

The `lib/portrait.lisp` library wraps the raw analysis APIs into
structured reports.

```text
(def portrait ((import "std/portrait.lisp")))

# Function portrait — signal profile, captures, callees
(println (portrait:render (portrait:function a :validate)))

# Module portrait — signal topology across all functions
(println (portrait:render (portrait:module a)))
```

## Advisories

A portrait reflects the compiler's lint diagnostics as advisories — it does not
re-derive them. Three rules reach a portrait:

| Rule | Advisory | What it says |
|---|---|---|
| `mutable-binding-never-assigned` | `:false-mutable` | A binding declared mutable (`var`/`@`) that no `assign` targets. |
| `unused-binding` | `:unused-binding` | A `def`/`let`/`letrec` binding nothing reads. |
| `non-tail-self-recursion` | `:non-tail-recursion` | A function whose self-call sits outside tail position. |

The false-mutable advisory surfaces the common conflation of a mutable
**binding** with a mutable **value**: `(let [buf @""] (push buf x))` mutates the
*value* but the *binding* never changes, so `buf` should stay immutable. Because
every advisory is read from `compile/diagnostics`, portrait and `elle lint`
always agree.

Each advisory appears at two granularities:

- **Module** — `(get (portrait:module a) :false-mutable)` lists every flagged
  binding across the module (including top-level ones). `:unused-binding` and
  `:non-tail-recursion` list theirs the same way.
- **Function** — `(portrait:function a :f)` includes one observation for each
  flag *inside* `f`. Per-function attribution is exact, not by line range: the
  linter tags each diagnostic with its nearest enclosing named function (the
  `:function` field on a diagnostic), and the observation filters on it. A flag
  in a nested closure is attributed to the inner function.

Adding a rule to a portrait is one row in `lint-kinds` (`lib/portrait.lisp`),
which both granularities read.

## Phases

1. **Analyze** — `compile/analyze` parses and type-checks without executing
2. **Query** — `compile/signal`, `compile/captures`, `compile/callees`
3. **Compose** — `portrait:function`, `portrait:module` build structured data
4. **Render** — `portrait:render` formats for display

---

## See also

- [signals](../signals/index.md) — signal system that portraits analyze
- [modules](../modules.md) — module structure
- [macros](../macros.md) — macro expansion before analysis
