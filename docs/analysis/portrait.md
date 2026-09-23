# Portrait

<!-- audited: 2026-09-22 -->

A portrait reports what the compiler knows about code without running it:
signals, captures, calls and lint advisories.

See [Agent Reasoning in Elle](agent-reasoning.md) for portraits and the MCP
server used together, and the [MCP server](../mcp.md) for queries across a
whole codebase.

## Analyze

`compile/analyze` expands, analyzes and lints source text, and returns a
handle. Nothing in the source runs.

```lisp
(def src "(defn validate [data]
  (when (nil? (get data :name))
    (error {:error :validation-error :message \"missing name\"}))
  data)

(defn process [items]
  (let [@count 0 @unused 1]
    (validate items)
    (fn [] (assign count (+ count 1)) count)))

(defn pick [b x y] (if b x y))

(defn count-down [n]
  (if (= n 0) 0 (+ 1 (count-down (- n 1)))))")

(def a (compile/analyze src {:file "example.lisp"}))
```

## Query

Each query takes the handle and, where it asks about one function, the
function's name as a keyword.

```lisp
# The inferred signal: its bits, and the parameters it takes a signal from.
(assert (contains? (get (compile/signal a :validate) :bits) :error))
(assert (get (compile/signal a :pick) :silent))

# Who calls a function, and what it calls.
(assert (= ["process"] (map (fn [c] (get c :name)) (compile/callers a :validate))))
(assert (= ["validate"]
           (filter (fn [n] (= n "validate"))
                   (map (fn [c] (get c :name)) (compile/callees a :process)))))

# The call graph: a node for each function that makes a call, and the
# leaves, which make none.
(def graph (compile/call-graph a))
(assert (= 3 (length (get graph :nodes))))
(assert (= ["pick"] (get graph :leaves)))
```

`compile/signal` also returns `:yields` and `:jit-eligible`. Both are derived
from a predicate that counts `:error`, so an error-only function reports
`:yields true` (#1234). Read `:bits` and `:propagates` instead.

The queries see the expanded program. A function that uses a macro such as
`each` reports the calls and bindings of the expansion as its own (#1235).

## Portrait library

[lib/portrait.lisp](../../lib/portrait.lisp) builds structured reports from
the queries. `portrait:function` describes one function; `portrait:module`
describes them all. `portrait:render` and `portrait:render-module` turn each
into text.

```lisp
(def portrait ((import "std/portrait")))

(def f (portrait:function a :validate))
(assert (= "validate" (get f :name)))
(assert (string/contains? (portrait:render f) "Effects:       error"))

(def m (portrait:module a))
(assert (string/contains? (portrait:render-module m) "Roots:"))
```

## Advisories

A portrait reports the linter's diagnostics as advisories. It never derives
them itself. Three rules reach a portrait:

| Rule | Advisory | What it says |
|---|---|---|
| `mutable-binding-never-assigned` | `:false-mutable` | A binding declared mutable (`var`/`@`) that no `assign` targets. |
| `unused-binding` | `:unused-binding` | A `def`/`let`/`letrec` binding nothing reads. |
| `non-tail-self-recursion` | `:non-tail-recursion` | A function whose self-call sits outside tail position. |

The false-mutable advisory catches a common mix-up of a mutable **binding**
with a mutable **value**. `(let [buf @""] (push buf x))` mutates the value,
but the binding never changes, so `buf` needs no `@`. Every advisory is read
from `compile/diagnostics`, so a portrait and `elle lint` always agree.

Each advisory appears at two granularities:

- **Module**: `(get (portrait:module a) :false-mutable)` lists every flagged
  binding in the module, top-level ones included. `:unused-binding` and
  `:non-tail-recursion` list theirs the same way.
- **Function**: `(portrait:function a :f)` holds one observation for each flag
  inside `f`. The linter tags each diagnostic with its nearest enclosing named
  function, and the observation filters on that tag. A flag in a nested
  closure belongs to the inner function.

```lisp
(defn flags? [advisories message]
  (any? (fn [d] (= message (get d :message))) advisories))

(assert (flags? (get m :false-mutable)
                "mutable binding 'unused' is never reassigned"))
(assert (flags? (get m :non-tail-recursion)
                "'count-down' calls itself outside tail position, so the stack grows with the recursion depth"))
(assert (flags? (get (portrait:function a :process) :observations)
                "binding 'unused' is never used"))
```

A new rule reaches a portrait as one row in `lint-kinds`
([lib/portrait.lisp](../../lib/portrait.lisp)), which both granularities read.

## See also

- [signals](../signals/index.md) — the signal system a portrait reports on
- [modules](../modules.md) — module structure
- [macros](../macros.md) — macro expansion, which runs before analysis
