# Portrait

<!-- audited: 2026-11-20 -->

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

(defn process [rows]
  (let [@count 0 @unused 1]
    (each row in rows (validate row))
    (fn [] (assign count (+ count 1)) count)))

(defn pick [b x y] (if b x y))

(defn send [conn msg] (conn:write msg))

(var hits 0)
(defn hit [] (assign hits (+ hits 1)))

(defn count-down [n]
  (if (= n 0) 0 (+ 1 (count-down (- n 1)))))")

(def a (compile/analyze src {:file "example.lisp"}))
(defn names [entries] (map (fn [e] (get e :name)) entries))
```

## Signals

Each query takes the handle and, where it asks about one function, the
function's name as a keyword.

`compile/signal` answers a function's inferred signal. `:bits` holds the
signals it raises, and `:propagates` the parameters whose signal it raises.
`:silent` is true when both are empty, and `:io` when `:bits` holds `:io`.
`:yields` is true when the function may suspend its fiber: it raises a
parameter's signal, or a bit other than `:error`, `:halt` and `:ffi`.

```lisp
(assert (contains? (get (compile/signal a :validate) :bits) :error))
(assert (not (get (compile/signal a :validate) :yields)) "an error suspends nothing")
(assert (get (compile/signal a :pick) :silent))
```

`compile/query-signal` answers the functions that match `:silent`, `:yields`
or a signal's keyword, each as `{:name :line}`. Any other query is an error.

```lisp
(assert (= ["pick"] (names (compile/query-signal a :silent))))
(assert (not (first (protect (compile/query-signal a :fast)))) "an unknown query")
```

## The program as written

The queries report the source as its author wrote it. What a macro's
expansion adds is not reported, and neither is a call the compiler makes for a
literal (`[..]`, `{..}`, `|..|`) or for a qualified name.

- A call that a macro template makes is not a callee. `each` places its body
  once for each kind of collection, and the call to `validate` there is one
  call.
- A call through a qualified name reports the name as written.
- A binding that a macro template introduces appears in no query, and raises
  no advisory. `compile/symbols` and `compile/bindings` name each binding the
  source introduces once, and no primitive.

```lisp
(assert (= ["validate" "+"] (names (compile/callees a :process))))
(assert (= ["conn:write"] (names (compile/callees a :send))))
(assert (= ["process"] (names (compile/callers a :validate))))
(assert (empty? (filter (fn [n] (contains? |"seq" "cur" "idx" "len"| n))
                        (names (compile/symbols a))))
        "the bindings of each are not the user's")
(assert (= ["validate"] (filter (fn [n] (= n "validate")) (names (compile/bindings a)))))
```

## Calls

`compile/callees` answers each call a function makes as `{:name :line :col
:tail}`, one for each call site. A callee carries `:signal` too, in the shape
`compile/signal` answers, when the analysis knows it: a function the source
defines, a primitive, or a standard-library function. `compile/callers`
answers each call site that calls the function, with the same keys and no
`:signal`. A call made in an anonymous function belongs to the named function
around it.

```lisp
(def validate-call (first (compile/callees a :process)))
(assert (= (get validate-call :signal) (compile/signal a :validate)))
(assert (nil? (get (first (compile/callees a :send)) :signal))
        "nothing says what a field of conn holds")
```

`compile/call-graph` answers `:nodes`, `:roots` and `:leaves`. A node stands
for each function that makes a call, and names each of its callers and callees
once. A root is a function the source defines that no other function calls. A
leaf is one that calls no other function the source defines.

```lisp
(def graph (compile/call-graph a))
(assert (= 5 (length (get graph :nodes))))
(assert (= ["count-down" "hit" "pick" "process" "send"] (get graph :roots)))
(assert (= ["count-down" "hit" "pick" "send" "validate"] (get graph :leaves)))
```

## Captures

`compile/captures` answers what a function closes over, each as `{:name :kind
:mutated}`. A reference to an immutable binding of the file is no capture,
because every function in the file shares the file. A reference to a `var` of
the file is one, because a closure can write it.

```lisp
(assert (empty? (compile/captures a :process)) "validate belongs to the file")
(assert (= ["hits"] (names (compile/captures a :hit))))
```

## Module exports

`compile/exports` reads the module surface off an analysis: the export
struct the file returns, plus the module closure's own shape. It is the
static half of a version-bump verifier — [modules](../modules.md) explains
why the returned struct literal is the public surface.

```lisp
(def modsrc (string "(fn [dep]\n"
                    "  (letrec [helper (fn [x] x)\n"
                    "           run (fn [job] \"Run one job.\" (helper job))]\n"
                    "    {:run run :limit 3}))"))
(def modana (compile/analyze modsrc {:file "mod.lisp"}))
(def ex (compile/exports modana))

# The constructor is the module closure's own shape.
(assert (= (get (get ex :constructor) :required) 1) "constructor arity")

# Each function export carries the record fn/signature would return,
# read statically: counts, rest kind, params, signals, doc, line.
(def run-rec (get (get ex :exports) :run))
(assert (= (get run-rec :kind) :fn) "run is a function")
(assert (= (get run-rec :required) 1) "run takes one argument")
(assert (= (get run-rec :params) ["job"]) "param names ride along")
(assert (= (get run-rec :doc) "Run one job.") "docstring rides along")

# A non-function export is recorded as a value.
(assert (= (get (get ex :exports) :limit) {:kind :value}) "limit is a value")

# A file that returns no export struct has no surface to report.
(assert (nil? (compile/exports (compile/analyze "(+ 1 2)"))) "no surface")
```

The exports map is keyed by export keyword, resolved through bindings —
a private helper sharing an export's name never shadows it. `:rest` and
`:named-keys` follow the vocabulary of `fn/signature`
([functions](../functions.md)); `:signals` has the shape `compile/signal`
returns.

## Rule-driven rewriting

`compile/apply-rules` drives the `elle rewrite` edit engine with rules
supplied as data — the consumer-migration half of `elle semver`
([semver](../semver.md)). Edits are token-level over the source text:
comments and formatting survive, and a quoted symbol renames like any
other token. The engine reapplies rules until the text stops moving, so
a rename inside a replaced call's arguments still lands.

```lisp
(def rules [{:kind :rename :from "m:old" :to "m:new"}
            {:kind :replace :name "m:bump" :arity 2
             :template "(m:increment $2 $1)"}
            {:kind :report :name "m:gone" :message "use m:new"}])
(def r (compile/apply-rules "(m:old (m:bump a b)) (m:gone 1)" rules))
(assert (= (r :source) "(m:new (m:increment b a)) (m:gone 1)")
        "renames and replaces apply together")
(assert (= (r :count) 2) "two edits")
(def rep (first (->list (r :reports))))
(assert (= (rep :name) "m:gone") "a report rule edits nothing")
(assert (= (rep :line) 1) "each occurrence carries its line")
(assert (= (rep :message) "use m:new") "and the shipped message")

# A replace at a different arity leaves the call alone, exactly like an
# epoch replace rule.
(assert (= ((compile/apply-rules "(m:bump a)" rules) :source) "(m:bump a)")
        "arity mismatch is not rewritten")
```

## Portrait library

[lib/portrait.lisp](../../lib/portrait.lisp) builds reports from the
queries. `portrait:function` describes one function: its signal, captures,
callers and callees, the calls that may raise an error, its composition, and
observations. `portrait:module` lists the silent functions under `:pure`,
beside the call graph and the advisories. A portrait sorts no other function
by what it does; the function's signal says that. `portrait:render` and
`portrait:render-module` turn each into text.

```lisp
(def portrait ((import "std/portrait")))

(def f (portrait:function a :validate))
(assert (= "validate" (get f :name)))
(assert (string/contains? (portrait:render f) "Effects:       error"))
(assert (any? (fn [x] (= (get x :source) "error")) (get f :failures))
        "a failure is a call whose callee may raise :error")

(def m (portrait:module a))
(assert (= ["pick"] (get m :pure)) "pure is silent")
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
from `compile/diagnostics`, so a portrait and `elle lint` always agree. A
finding that an expansion repeats in each copy of a body is one diagnostic.

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
