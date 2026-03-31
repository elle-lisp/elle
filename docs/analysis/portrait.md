# Portrait

The portrait system exposes everything the compiler knows about your code:
signal profiles, capture analysis, composition properties, and the call graph.
It analyzes source without executing it.

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
(def portrait ((import "lib/portrait.lisp")))

# Function portrait — signal profile, captures, callees
(println (portrait:render (portrait:function a :validate)))

# Module portrait — signal topology across all functions
(println (portrait:render (portrait:module a)))
```

## Phases

1. **Analyze** — `compile/analyze` parses and type-checks without executing
2. **Query** — `compile/signal`, `compile/captures`, `compile/callees`
3. **Compose** — `portrait:function`, `portrait:module` build structured data
4. **Render** — `portrait:render` formats for display

---

## See also

- [signals.md](signals.md) — signal system that portraits analyze
- [modules.md](modules.md) — module structure
- [macros.md](macros.md) — macro expansion before analysis
