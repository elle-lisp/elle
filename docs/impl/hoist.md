# Hoisting constants

<!-- audited: 2026-11-17 -->

A constant that a function or a loop reads is built once, when its compile unit starts, and every call and pass reads that one value.

A heap literal is an ordinary allocation, built fresh each time it runs
([the region model](region/model.md)). In a function body that is once per
call, in a loop body once per pass, and each build claims a region page of its
own. So the compiler moves each such constant into one `let` around the root
of the unit, and every function between that `let` and the constant captures
it. The region solver then treats the constant as it treats any binding a
closure captures.

```lisp
(defn fhir [src]
  "The functionalized HIR of src, as text."
  (get (compile/dumps src "<doc>") :fhir))

(defn before-fn? [text part]
  "Whether part first occurs in text before the first function does."
  (< (string/find text part) (string/find text "(fn ")))

(def unit
  (fhir (string "(defn space? [g] (= g \" \")) (defn buf [] @\"x\")"
                " (def greeting \"hi\")"
                " (defn f [x] (if (nil? x) (error \"no item\") x))"
                " [space? buf f]")))

(assert (before-fn? unit "\" \"") "the string is built before any function")

(defn space? [g] (= g " "))
(assert (= (fn/captures space?) 1) "space? captures the string it compares with")
```

## What a constant is

| Form | A constant when |
|------|-----------------|
| A string, `"…"` | always |
| A quoted datum, `'(…)` or `'[…]` | no part of it is mutable |
| `[…]`, `{…}`, `\|…\|` or `b[…]` | every item is an immediate or a constant, none is spliced, and the literal is silent |

A collection literal is silent when the analyzer checks each of its items at
compile time. A silent literal cannot raise, so building it at the start of
the unit changes nothing but when it is built.

A mutable literal is never a constant. `@"…"`, `@[…]`, `@{…}` and `@|…|` build
a new value on every run ([syntax](../syntax.md)). The constants inside one
still move: `@"x"` thaws the string `"x"`, so the string moves and the thaw
stays where it was.

```lisp
(assert (before-fn? unit "\"x\"") "the string the thaw copies moves")
(assert (not (before-fn? unit "%thaw#")) "the thaw stays in buf")
```

## Where a constant goes

A constant moves when a function body or a loop body encloses it. It moves to
a fresh immutable binding of the `let` at the root of the unit, and a read of
that binding replaces it.

- The root of a file is its file `letrec`. The root of a REPL line or an
  `eval` is that form. The root is the one point outside every function that
  comes before every closure is built.
- A constant that neither a function nor a loop encloses stays, because it runs
  once already. That covers a top-level `def`, and the initial values of a
  loop outside every function.
- A constant that only a raise reads stays. The value of an `(emit :error …)`
  is built on the run that raises, which is at most one run. Every `error` is
  such an emit, and so is the guard a grafted wrapper raises from
  ([grafting a call](graft.md)).
- Each function between the `let` and the constant captures the binding, after
  the captures it had. The binding is immutable and has no forward reference,
  so the capture holds the value and no cell. The graft adds a capture the same
  way ([the captures a graft adds](graft/captures.md)).
- Equal constants share one binding. Two constants are equal when they hold the
  same contents, a float compared by its bits, so `0.0` and `-0.0` stay apart.
  A syntax symbol of a macro template also compares by its scopes and its span.
  An immutable value compares by its contents, under `identical?` as under `=`
  ([types](../types.md)), so no program can tell one shared value from two.
- The fresh binding holds its constant for as long as the unit runs. Region
  inference counts it as a holder of that value, as it counts every binding
  but an ANF temp ([the ANF lift](anf.md)). So a `var` that starts from a
  moved constant takes a reference of its own, which its first `assign` drops
  ([bindings](region/bindings.md)).

```lisp
(assert (not (before-fn? unit "\"no item\"")) "a raise's constant stays")
(assert (< (string/find unit "(letrec") (string/find unit "\"hi\""))
        "a top-level def's constant stays")

(def shared (fhir "(defn a [g] (= g \"|\")) (defn b [g] (= g \"|\")) [a b]"))
(assert (= (length (string/split shared "\"|\"")) 2)
        "two functions that read one string read one binding")
```

A `let` around the root, and not around each function, keeps every binder as
it is. A `def`, a file `letrec` binding and a local `defn` each read a lambda
init directly: the lowerer names the function from it, the self-reference
comes from it, and the closure-cycle merge reads it
([letrec](region/letrec.md)).

A macro body's quasiquote template is a constant like any other, so every
expansion reads one value. The expander copies a syntax value before it uses
it, so no expansion changes the value another one reads.

## What a program sees

A program sees the time and the memory, and nothing else. A constant that
leaves a function, returned or pushed into an array, is the one shared value.
Reference counting holds it for as long as anything holds it.

```lisp
(defn greeting [] "hello")
(def kept @[])
(push kept (greeting))
(push kept (greeting))
(assert (= (freeze kept) ["hello" "hello"]) "each returned copy keeps its value")
```

A `var` that starts from a constant and is then reassigned leaves the constant
as it was. The next call or pass starts from the same value.

```lisp
(defn pairs-twice [xs]
  "xs in runs of two, taken twice; each run starts from the literal []."
  (var out [])
  (each _ in [1 2]
    (var run [])
    (each x in xs
      (when (= (length run) 2)
        (assign out (append out [run]))
        (assign run []))
      (assign run (append run [x]))))
  out)

(assert (= (pairs-twice [1 2 3 4 5]) [[1 2] [3 4] [1 2] [3 4]])
        "every pass starts its run from the empty array")
(assert (= (pairs-twice [1 2 3]) [[1 2] [1 2]]) "and so does every call")
```

## What it costs

- Each distinct constant is a region of its own, built when the unit starts
  and held while a function that reads it lives. So a unit claims one page per
  distinct constant at its start ([#1496](https://github.com/elle-lisp/elle/issues/1496)
  asks for one page for all of them).
- A constant in a branch that never runs is built all the same. The pass treats
  a raise as the one branch it knows is cold, which is why a raise keeps its
  constants.
- A closure holds one environment slot per constant it captures, and
  `fn/captures` counts each one. A closure built in a loop copies one value
  more per constant each time it is built.
- The entry function of the unit holds one local slot per moved constant. A
  function past the slot limit is a compile error
  ([head.rs](../../src/lir/build/head.rs)).

A per-unit pool of constants tied to the code object is the alternative that
lost: the region model forbids such a pool. A `let` around each function's own
site lost too, because it breaks the binders above.

## Where it runs

`hoist_constants` ([hoist.rs](../../src/hir/hoist.rs)) runs after
functionalization and before the ANF lift, at every entry that runs the two.
After functionalization every `while` is a `loop`, so the pass reads one form
of loop. Before the ANF lift, a moved constant is the init of a `let`, a binder
position that the `let`'s own slot names ([the ANF lift](anf.md)).

The pass rewrites the tree in place. It keeps the id of every node but a
constant equal to one it moved before, whose node leaves the tree. The operand
record of the graft names only variable reads, so every node it names stays in
the tree.

## Pinning tests

- `hir::hoist::tests::*` — what moves and where it lands. A string, an
  immutable quoted datum and a silent collection literal of constants in a
  function or a loop move to the `let` at the root, and every function around
  each one captures its binding. Equal constants share one binding, and `0.0`
  and `-0.0` do not. A mutable literal, a constant a raise reads, and a
  constant outside every function and loop stay.
- [the function-literal producer](../../tests/impl/region-fn-literals.lisp) —
  a function that a loop calls through a parameter, and that compares its
  argument with a string literal or looks it up in a set literal, claims no
  page per call. It takes the time of the same function over a global.
- [the loop-literal producer](../../tests/impl/region-loop-literals.lisp) — a
  loop that reads a string, set or struct literal claims no page per entry, and
  takes the time of the same loop over a global.
- [the function-literal UAF test](../../tests/impl/region-fn-literal-uaf.lisp)
  — under guardfree, a constant returned, pushed, reassigned from, captured by
  a closure built in a loop, read by mutually recursive functions, or read by a
  macro keeps its value on every call.
- [the literal language test](../../tests/lang/literal-fresh.lisp) — a mutable
  literal in a loop or a function is a new value on every pass and call, and
  an immutable one keeps its value after it leaves.
- [the var language test](../../tests/lang/literal-var.lisp) — a `var` that
  starts from a literal starts from that literal on every pass and call,
  however often it is reassigned.
- [the macro language test](../../tests/lang/literal-macro.lisp) — a macro
  whose template holds constants expands the same way every time.
- [the slot-limit test](../../tests/impl/hoist-slot-limit.lisp) — a unit with
  more distinct constants than the entry function has local slots fails to
  compile, with an error that names the limit.
- `hir::region::infer::tests::reassign::constant::*` — a reassigned `var`
  whose init the hoist moved counts that init, and suppresses no release of it.
