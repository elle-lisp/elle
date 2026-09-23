# Design Philosophy

<!-- audited: 2026-09-22 -->

Why Elle infers signals instead of asking for them, and the gap that leaves
between what the compiler knows and what a reader sees.

## Signals are inferred

The compiler infers a signal for every function. A function whose body cannot
signal is silent with no annotation. A function that uses generic arithmetic
may raise `:error`, because `+` rejects a non-number. A function that calls a
parameter takes its signal from the argument: it is polymorphic.

```lisp
(def a (compile/analyze "
(defn pick [b x y] (if b x y))
(defn add [x y] (+ x y))
(defn call [f x] (f x))"))

(assert (get (compile/signal a :pick) :silent) "pick is silent, unannotated")
(assert (= |:error| (get (compile/signal a :add) :bits)) "add may raise :error")
(assert (= |0| (get (compile/signal a :call) :propagates))
        "call takes its signal from parameter 0")
```

The signal decides the calling convention. A call whose callee may yield, do
I/O or wait compiles to a suspending call, which keeps a continuation frame so
that the fiber can resume it. Any other call is a plain call.

## Higher-order functions are polymorphic by default

A parameter that is called carries no bound unless the function declares one.
This is a deliberate choice for Elle's main use: concurrent programs built
from fibers and asynchronous I/O, where most callbacks do I/O.

The alternative bounds every callback to silent and asks for an opt-in
wherever a callback may yield. In a concurrent program that opt-in lands on
most callbacks, at every `map`, every `ev/spawn` and every handler. Elle
charges the annotation to the minority case instead: the code that must not
suspend.

`(silence f)` bounds the parameter `f`. The function is then silent with
respect to `f`, and a closure that may signal fails a check at entry:

```lisp
(defn apply-silent [f x]
  (silence f)
  (f x))

(assert (= 42 (apply-silent (fn [x] x) 42)) "a silent closure passes")

(def [ok? err] (protect (apply-silent (fn [x] (yield x)) 42)))
(assert (not ok?) "a yielding closure fails the entry check")
(assert (= :signal-violation (get err :error)))
```

## Silence means no signal at all

`(silence)` with no argument declares that the function emits nothing,
`:error` included. It fits a body that cannot fail: branching, locals, and
the `%` intrinsics on values already known to be numbers. The compiler
rejects a `(silence)` body that may signal.

```lisp
(defn select [flag a b]
  (silence)
  (if flag a b))

(assert (= 1 (select true 1 2)))

(def [ok? err] (protect (eval '(fn [x y] (silence) (+ x y)))))
(assert (not ok?) "generic + may raise :error, so (silence) rejects it")
(assert (string/contains? (get err :message) "body may emit {:error}"))
```

A function that may fail but must not suspend declares `(attune! :error)`,
which sets the ceiling to `:error` alone. See
[signals/inference.md](signals/inference.md) for every declaration form.

## The gap is visibility

The compiler knows every signal, and a reader sees none of them.
Nothing in the source text shows whether a function is silent, may fail, or
may suspend.

1. **Hidden cost.** A function that reads as a tight loop can call something
   that may yield. Each such call then compiles to a suspending call, and
   nothing in the text says so.
2. **No marker.** No syntax, gutter icon or highlight shows a function's
   signal. A developer finds out from a compile error, or from a profile.
3. **Late hardening.** Making code silent after it grew flexible is slow
   work. The developer finds out late which callees widened the signal, much
   like adding type annotations to Python code after the fact.

The compiler already computes every signal. `compile/signal` returns it for
one function, as the examples above show. The [portrait](analysis/portrait.md)
system and the [MCP server](mcp.md) present it as data a tool or an agent can
query. See [Agent Reasoning in Elle](analysis/agent-reasoning.md).

## Reasoning about a call

- A function that calls a parameter is polymorphic, unless it bounds that
  parameter with `(silence f)`.
- A function that does I/O, or uses fibers through `ev/spawn` or `ev/join`,
  may yield.
- A function's signal is at least the union of its callees' signals.
- Generic arithmetic, `get` and `assert` may raise `:error`.

Declare `(silence)` where the contract matters to callers, or where a hot path
must not suspend. It is a contract, not a default.

## See also

- [Module system](modules.md) — the constraints of the module system and
  their reasons
- [Signal inference](signals/inference.md) — how signals are inferred,
  bounded and checked
- [Agent Reasoning](analysis/agent-reasoning.md) — how agents analyze and
  refactor Elle code
- [MCP Server](mcp.md) — the semantic knowledge graph and its query interface
