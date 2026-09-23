# Grafting a call

<!-- audited: 2026-09-23 -->

A call into another unit's function becomes that function's body, and the proven types delete every branch its operands cannot take.

A closure is the unit of nothing at run time; the fiber is the unit a program
controls ([dissolution](dissolution.md)). So the compiler does not keep a call
into a function an earlier unit defined when it can see that function's body.
It grafts the body in place of the call, and after type inference it folds away
the guards and dispatch arms the operands' proven types decide. A stdlib wrapper
over proven operands ends as the one `%`-op its body reaches:

```lisp
(defn fhir [src]
  "The functionalized HIR of src, as text."
  (get (compile/dumps src "<doc>") :fhir))

(defn occurrences [text part]
  "How many times part occurs in text."
  (- (length (string/split text part)) 1))

# Every caller passes an int, so the operands are proven and only %add is left.
(def proven (fhir "(defn h [a b] (+ a b))\n(h 1 2)"))
(assert (= 1 (occurrences proven "(%add ")) "the op survives")
(assert (= 0 (occurrences proven "+#")) "the call to + is gone")
(assert (= 0 (occurrences proven "(emit ")) "and so are both guards")

# One caller passes a keyword. The body is still grafted, but the guards stay,
# and they are what prove the %add operands.
(def unproven (fhir "(defn h [a b] (+ a b))\n(h 1 2)\n(h :k 2)"))
(assert (= 0 (occurrences unproven "+#")) "the call to + is gone")
(assert (= 1 (occurrences unproven "(%add ")))
(assert (< 0 (occurrences unproven "(emit ")) "the guards raise at run time")

# A container wrapper takes the same path: push on a proven @string is one op.
(def pushed (fhir "(defn g [] (let [s @\"\"] (push s \"x\") s))\n(g)"))
(assert (= 0 (occurrences pushed "push#")))
(assert (= 1 (occurrences pushed "%string-push-mut")))
```

The two passes are the graft, before functionalization, and the fold, after the
inference fixpoint. Neither names a function: what `+` means is what `+`'s body
says, read from the body.

## The graft

### Which calls

A call grafts when all of these hold:

- Its callee is a `Var` naming a binding an earlier unit exports, whose value
  carries its body (see "Where the body comes from").
- No operand is spliced, so the count of operands is known.
- The function takes fixed parameters and at most a `&` rest list; `&opt`,
  `&keys` and `&named` decline. No parameter is mutated in the body.
- The call's operand count selects a body that closes as an `HirFragment`
  ([the HIR design](hir.md)). A body that holds a `lambda`, a `letrec`, a loop,
  an `assign` or a `define` does not close, and its calls stay calls.
- The function does not call itself. Nor does the graft graft a function
  inside its own graft, which bounds a cycle that runs through several
  functions.

A call that fails any of these stays the call it was. Nothing else is asked:
there is no declaration and no list of names.

A function this unit defines is not grafted. Its parameters take their types
from the calls this unit makes to it ([type inference](typeinfer.md)). Grafting
those calls away would leave its definition with no call site to prove its
parameters, and a `%`-op in its body that one of those calls proved would stop
compiling. The calls a function's own body makes are grafted like any other,
so `(+ a b)` inside a function whose callers pass ints still dissolves.

### Where the body comes from

A function from another unit is reached through its value. When a unit is
analyzed, each binding the earlier units export maps to its compile-time value,
and for a stdlib function that value is a closure. A closure's template carries
the fragment of its own body, closed before functionalization in the unit that
defined it and stored when the lambda lowers. The graft reads the fragment off
the value the binding holds. A user binding that shadows `+` holds a different
value, so it can never reach `+`'s body.

The stdlib disk cache stores each template's fragment with the template
([the stdlib cache](stdlib-cache.md)). A template hydrated from an image carries
none yet, so a call to it stays a call ([booting from an image](image/boot.md)).

### Operands and parameters

The graft binds each parameter to its operand in the order the operands are
written, so the evaluation order of the call is kept. An operand that is an
immediate literal, or a `Var` naming an immutable, unmutated binding, replaces
the parameter directly in the body. Every other operand is bound by a `let` to a
fresh local first. The direct replacement matters for the fold: a guard in the
grafted body then tests the caller's own binding, and its fact narrows that
binding.

A parameter a `(numeric!)` declaration floors is always bound by a `let`. The
floor is a fact about the parameter's binding, and the `let` carries that
binding into the caller, so the body's `%`-ops prove against the floor as they
did inside the function. Replacing the parameter with an unproven operand would
drop the floor and reject a program that compiles today.

### The rest list and the known arity

A rest parameter receives the operands past the fixed ones. The count is known
at the call, so a body that dispatches on the rest list decides the dispatch at
graft time. The body must be a `match` on the rest parameter. Its arms are tried
in order against the known shape of the list:

- `()` matches no extra operands;
- a list pattern of plain variables or `_`, as `(b)` or `(a b)`, matches that
  many, and a `& r` tail matches that many or more;
- `_` or a bare variable matches any count.

The first arm that matches is the body the graft uses, and its pattern variables
bind to the operands under the rule above. A tail or a bare variable binds to a
`(list …)` of the operands it covers. An arm with a literal element, a nested
pattern or a guard cannot be decided at graft time, so reaching one declines the
graft. A body that is not such a `match` binds the rest parameter to the
`(list …)` of the extra operands, `()` when there are none.

Each arm closes on its own. An arm that does not close, such as the general arm
of `+` with its `letrec` walker, leaves the calls of its arity as calls, while
the other arities still graft.

## The fold

After the inference fixpoint the fold rewrites what the recorded types decide.
It then hands the tree back to inference, and the two alternate until a fold
changes nothing. The deletions can narrow a join, and a later guard or op may
prove only against the narrower type. The contract check, the signal narrowing
and the signal re-propagation run on the tree the last fold left
([type inference](typeinfer.md)).

### A decided condition

A condition is decided when it is built only from literals, `%not`, `and`,
`or`, `let` bindings of such expressions, and type tests of an operand. A type
test is a `%`-predicate, a call to a predicate in the vocabulary guard
recognition reads, or a `(match (type-of x) …)` whose arms are literals. The
test answers from the operand's recorded type `t` and the tested type `T`:
`subtype(t, T)` is true, `meet(t, T)` of Bottom is false, and anything else is
undecided. A `type-of` match compares keyword sets instead, which also decides
the two mutabilities of a string or an array apart.

Every form in that grammar is free of effects, so a decided condition is removed
whole. An `if` becomes its taken branch. A `(match (type-of x) …)` whose live
arm is decided becomes that arm's body.

The fold asks no question about where the code came from. A branch the
programmer wrote is deleted exactly as a grafted one is, so a site inside a
guard that contradicts its operand's type is gone before the contract check,
rather than rejected by it:

```lisp
(defn compiles? [src]
  "Whether src compiles through the file front end."
  (first (protect (compile/whole-module src "<doc>"))))

(assert (compiles? "(defn g [b] (if (%int? b) (%bit-and b 1) 0)) (g \"str\")")
        "no string reaches %bit-and, so the branch holding it goes")
```

### Certain divergence

A guard whose condition is decided true and whose branch diverges ends the
sequence it sits in. The fold deletes the statements after it. Without this
rule, `(+ "a" 1)` would stop compiling: the guard's fall-through fact meets a
string with Number, the narrowing hands `%add` a Bottom operand, and the
contract rejects the site. The fold and the narrowing read one lattice, so every
Bottom the narrowing makes is a condition the fold decides first.

```lisp
(defn raises? [thunk]
  "Whether calling thunk raises."
  (not (first (protect (thunk)))))

(assert (raises? (fn [] (+ "a" 1))) "the program compiles, and raises at run time")
```

### What a deletion may not take

A deleted branch or arm must not bind a name that code outside it reads.
Functionalization can move a binding's cell out of the arm that introduced it,
and deleting that arm would orphan the cell. The fold leaves such a branch in
place.

### Signals

A deleted guard takes its `emit` with it, and the signal re-propagation then
drops the site's error bit. The signal the analyzer recorded for the enclosing
function does not change, so `(silence)` accepts and refuses exactly the
programs it did before.

## Guard facts from a grafted predicate

A predicate such as `number?` is itself a stdlib function, so a guard's call to
it is grafted too. Guard recognition reads the grafted form: a
`(match (type-of x) …)` whose arms are `true` and `false` literals proves, when
truthy, the join of the types whose arms answer `true`. It proves that only when
the join is exact in the lattice: `:integer` and `:float` join to Number, which
is exact, while `:string` and `:@string` join to Top, which proves nothing.

## What a program sees

- A guard that raises in a grafted body raises the same error value, from the
  caller's frame. A backtrace no longer shows the wrapper's frame.
- A grafted call to a variadic wrapper allocates no rest list, so an exact
  allocation count moves.
- `(fn/disasm f)` shows the grafted code.

## The wrappers' shapes

The arithmetic wrappers dispatch on the rest list. The two-operand arm is two
number guards and one `%`-op, and the general arm keeps its `letrec` walker:

```lisp
(assert (= (+) 0) "no operands")
(assert (= (+ 1 2) 3) "the two-operand arm")
(assert (= (+ 1 2 3) 6) "the general arm")
(assert (= (string (+ -0.0 -0.0)) "-0.0") "the sum starts from its first operand")
```

`+` sums from its first operand, not from `0`, because `0 + -0.0` is `0.0`
while `%add` keeps the negative zero. The comparison wrappers take a two-operand
arm that tests for two numbers and reaches `%lt` and its peers, and otherwise
validates with `check-comparable` and compares through the op as a value. `=`
is a stdlib wrapper of the same shape over `%eq`. The number test stays in both
until the WASM tier compares strings by content (#1268). The container wrappers
`push`, `put`, `add` and `del` dispatch on `(type-of coll)`, and `put` takes its
optional value through the rest list.

## Pinning tests

- `hir::typeinfer::tests::graft::*` — which calls graft: each arithmetic and
  comparison wrapper over two proven operands leaves one op and no guard, an
  unproven pair keeps its guards, three operands and a spliced operand keep the
  call, a user binding named `+` keeps its call, a recursive function stays a
  call, and a call to a function this unit defines stays a call.
- `hir::typeinfer::tests::fold::*` — what the fold decides: a guard over a
  proven operand, a `type-of` dispatch over a proven container, the certain
  divergence of `(+ "a" 1)`, and the fact a grafted predicate proves.
- [wrapper-lowering.lisp](../../tests/impl/wrapper-lowering.lisp) — each wrapper computes one value in its
  grafted and its called spelling, over ints, floats, both zeros, NaN,
  infinities and the overflow edge.
- [typed-int-ops.lisp](../../tests/impl/typed-int-ops.lisp) — the wrapper-spelled `fib` emits `AddInt`
  and `SubInt`.
