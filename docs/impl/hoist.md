# Hoisting a loop's constants

<!-- audited: 2026-11-13 -->

A constant that a loop body reads is built once, before the loop, and every pass of the loop reads that one value.

A heap literal is an ordinary allocation, built fresh each time it runs
([the region model](region/model.md)). Inside a loop that is once per pass,
and each build claims a region page of its own. So the compiler moves each
constant out of the loop body, into a `let` around the loop. The region solver
then treats the constant as it treats any binding a loop reads: one region,
made when the loop is entered and released after its last pass
([last use](region/lastuse.md)).

```lisp
(defn fhir [src]
  "The functionalized HIR of src, as text."
  (get (compile/dumps src "<doc>") :fhir))

(defn scan-of [form]
  "The FHIR of a function whose loop tests each item of gs with form."
  (fhir (string "(defn f [gs] (var i 0) (while (and (< i 3) (not " form
                ")) (assign i (inc i))) i) (f [\"a\" \"b\" \"c\"])")))

(defn before-loop? [text part]
  "Whether part first occurs in text before the loop does."
  (< (string/find text part) (string/find text "(loop ")))

(assert (before-loop? (scan-of "(= (gs i) \"|\")") "\"|\"")
        "the string is built before the loop")
(assert (before-loop? (scan-of "(has? |\"|\" \"-\"| (gs i))") "(set#")
        "so is the set, with the strings it holds")
```

## What a constant is

| Form | A constant when |
|------|-----------------|
| A string, `"…"` | always |
| A quoted datum, `'(…)` or `'[…]` | no part of it is mutable |
| `[…]`, `{…}`, `\|…\|` or `b[…]` | every item is an immediate or a constant, none is spliced, and the literal is silent |

A collection literal is silent when the analyzer checks each of its items at
compile time. A silent literal cannot raise, so building it before the loop
changes nothing but when it is built.

A mutable literal is never a constant. `@"…"`, `@[…]`, `@{…}` and `@|…|` build
a new value on every run ([syntax](../syntax.md)). The constants inside one
still move: `@"x"` thaws the string `"x"`, so the string moves out and the thaw
stays in the loop.

```lisp
(def thawed (scan-of "(= (gs i) @\"|\")"))
(assert (before-loop? thawed "\"|\"") "the string the thaw copies moves out")
(assert (not (before-loop? thawed "(%thaw#")) "the thaw stays in the loop")
```

## Where a constant goes

The pass walks each function body. At a loop that no other loop of the same
function encloses, it binds each constant of the loop body to a fresh
immutable binding. A read of that binding replaces the constant, and a `let`
of all of them wraps the loop.

- The fresh binding holds its constant for every pass. Region inference counts
  it as a holder of that value, as it counts every binding but an ANF temp
  ([the ANF lift](anf.md)). So a `var` that starts from a moved constant takes
  a reference of its own, which its first `assign` drops
  ([bindings](region/bindings.md)).
- A nested loop's constants go out with the outermost loop's. The outer body
  runs the inner loop on every pass, so a constant left there would be built
  on every pass of the outer loop.
- A loop's initial values run once on each entry, so they stay where they are.
- A constant inside a `fn` in the loop stays in the `fn`. The `fn` is a
  function of its own, and the pass moves the constants of its loops out of
  those loops.
- A constant that only a raise reads stays where it is. The value of an
  `(emit :error …)` is built on the pass that raises, which is at most one
  pass. Every `error` is such an emit, and so is the guard a grafted wrapper
  raises from ([grafting a call](graft.md)).

```lisp
(def closed (scan-of "(= (gs i) (fn [] \"|\"))"))
(assert (not (before-loop? closed "\"|\"")) "a constant in a fn stays there")

(def raised (scan-of "(if (nil? (gs i)) (error \"no item\") false)"))
(assert (not (before-loop? raised "\"no item\"")) "a raise's constant stays")
```

## What a program sees

A program sees the time and the memory, and nothing else. An immutable value
compares by its contents, under `identical?` as under `=`
([types](../types.md)). So no program can tell one value read on every pass
from a value built per pass. A constant that leaves the loop, pushed into an
array for example, is that one value. Reference counting holds it for as long
as anything holds it.

```lisp
(defn tags [n]
  "An array of n copies of one literal, filled in a loop."
  (def out @[])
  (var i 0)
  (while (< i n) (push out "tag") (assign i (inc i)))
  out)

(assert (= (freeze (tags 3)) ["tag" "tag" "tag"]) "each copy outlives the loop")
```

A `var` that starts from a constant and is then reassigned leaves the constant
as it was. The next pass starts from the same value.

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
```

## What it costs

A moved constant is built once on each entry to the loop. A constant in a
branch the loop never takes is built on entry all the same, so a loop that
never takes that branch pays one region per entry where it paid none. The pass
treats a raise as the one branch it knows is cold, which is why a raise keeps
its constants.

## Where it runs

`hoist_loop_constants` ([hoist.rs](../../src/hir/hoist.rs)) runs after
functionalization and before the ANF lift, at every entry that runs the two.
After functionalization every `while` is a `loop`, so the pass reads one form
of loop. Before the ANF lift, a moved constant is the init of a `let`, a binder
position that the `let`'s own slot names ([the ANF lift](anf.md)).

The pass rewrites the tree in place. It keeps the id of every node it does not
move, so the operand record of the graft still names nodes in the tree.

## Pinning tests

- `hir::hoist::tests::*` — what moves and where it lands. A string, an
  immutable quoted datum and a silent collection literal of constants move to a
  `let` around the outermost loop. A mutable literal, a constant in a `fn`, a
  constant a raise reads and a loop's initial value stay.
- [the loop-literal producer](../../tests/impl/region-loop-literals.lisp) — a
  loop that compares each item with a string literal, or looks it up in a set
  or struct literal, claims one page per entry for each value the literal
  builds. It takes the time of the same loop over a global. A literal pushed
  into an array on every pass is one region, freed when the array is.
- [the literal language test](../../tests/lang/literal-fresh.lisp) — a mutable
  literal in a loop is a new value on every pass, and an immutable one keeps
  its value after it leaves the loop.
- [the var language test](../../tests/lang/literal-var.lisp) — a `var` that
  starts from a literal in a loop starts from that literal on every pass. That
  holds in a single loop and in a nested one, however often the pass reassigns
  it.
  [The guardfree twin](../../tests/impl/region-hoisted-init-uaf.lisp) faults
  where a freed constant would read correctly until its memory is reused.
- `hir::region::infer::tests::reassign::constant::*` — a reassigned `var`
  whose init the hoist moved counts that init, and suppresses no release of it.
