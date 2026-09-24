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

(defn compiles? [src]
  "Whether src compiles through the file front end."
  (first (protect (compile/whole-module src "<doc>"))))

# Every caller passes an int, so the operands are proven and only %add is left.
(def proven (fhir "(defn h [a b] (+ a b))\n(h 1 2)"))
(assert (< 0 (occurrences proven "(%add ")) "the op survives")
(assert (= 0 (occurrences proven "+#")) "the call to + is gone")
(assert (= 0 (occurrences proven "(emit ")) "and so are both guards")

# One caller passes a keyword. The body is still grafted, but the guards stay,
# and they are what prove the %add operands.
(def unproven (fhir "(defn h [a b] (+ a b))\n(h 1 2)\n(h :k 2)"))
(assert (= 0 (occurrences unproven "+#")) "the call to + is gone")
(assert (< 0 (occurrences unproven "(%add ")))
(assert (< 0 (occurrences unproven "(emit ")) "the guards raise at run time")

# A container wrapper takes the same path: push on a proven @string is one op.
# g grafts at the call (g) as well, so the op stands in g and at the call.
(def pushed (fhir "(defn g [] (let [s @\"\"] (push s \"x\") s))\n(g)"))
(assert (= 0 (occurrences pushed "push#")))
(assert (= 2 (occurrences pushed "%string-push-mut")))
```

The two passes are the graft, before functionalization, and the fold, after the
inference fixpoint. Neither names a function: what `+` means is what `+`'s body
says, read from the body.

## The graft

### Which calls

A call grafts when all of these hold:

- Its callee is a `Var` naming an immutable, unmutated function binding whose
  body the compiler holds (see "Where the body comes from").
- No operand is spliced, so the count of operands is known.
- The function takes fixed parameters and at most a `&` rest list; `&opt`,
  `&keys` and `&named` decline. No parameter is mutated in the body.
- The call's operand count selects a body that closes as an `HirFragment`
  ([the HIR design](hir.md)). A body that holds a `lambda`, a `letrec`, a loop,
  a `block`, an `assign` or a `define` does not close, and its calls stay
  calls.
- The selected body does not recurse. Recursion is judged per arm. An arm of a
  function this unit defines recurses when it calls the function again,
  directly or through a cycle of this unit's functions. An arm of another
  unit's function recurses when it calls the function's own name. Only a call
  with no spliced operand counts, because only such a call could graft again.
  Nor does the graft graft a function inside its own graft, which bounds a
  cycle that runs through several units.
- The selected body propagates no parameter's signal. Each node of a grafted
  body keeps the signal inferred where the function was defined, and a
  parameter's position means nothing at the call site.
- Inside a lambda, every binding of this unit that a same-unit body names is
  one the lambda captures or binds. The analyzer fixed the captures before the
  graft runs, so a body that names another binding stays a call there.
- The call sits in live code (see "Code nothing reaches").

A call that fails any of these stays the call it was. Nothing else is asked:
there is no declaration and no list of names. A stdlib function and a function
the program defines graft alike.

### Code nothing reaches

Code outside every lambda is live. A lambda written in live code is live,
unless it initializes a binding: then it is live when live code names that
binding, as a callee or as a value. A lambda that nothing live reaches holds
no call the graft replaces.

Such a function's parameters take no type from any call, so they prove nothing
([type inference](typeinfer.md)). A body grafted there would put the callee's
ops over operands that prove nothing. The same ops in the callee's own
definition prove against the callers that do exist. Below, `go` calls `helper`
and nothing calls `go`, so the program compiles only while that call stays a
call:

```lisp
(assert (compiles? "(defn plain [n]
                      (letrec [helper (fn [x] (%sub x 1))
                               go (fn [m] (helper m))]
                        (helper n)))
                    (plain 5)")
        "the call in go stays a call, and helper's x is proven by (helper n)")
```

### Where the body comes from

A function this unit defines is in the tree being compiled. The graft closes its
initializer's body into a fragment before it grafts any call.

A function from another unit is reached through its value. When a unit is
analyzed, each binding the earlier units export maps to its compile-time value,
and for a stdlib function that value is a closure. A closure's template carries
the fragment of its own body, closed before functionalization in the unit that
defined it and stored when the lambda lowers. The graft reads the fragment off
the value the binding holds. A user binding that shadows `+` holds a different
value, so it can never reach `+`'s body.

Such a fragment names its free globals by name. The graft resolves each one
among the bindings the runtime supplies to this unit: the primitives and the
stdlib's exports. A global that does not resolve keeps the call a call, so the
stdlib exports every helper a wrapper's body names, such as `dynamic-lt` and
`check-comparable`.

The stdlib disk cache stores each template's fragment with the template
([the stdlib cache](stdlib-cache.md)). A template hydrated from an image carries
none yet, so a call to it stays a call ([booting from an image](image/boot.md)).

### A grafted call still proves the definition

A function this unit defines keeps its definition, and its parameters take their
types from the calls this unit makes to it ([type inference](typeinfer.md)).
Grafting a call removes it from the tree, so the graft records what the call
would have contributed. For each parameter it notes the operand it bound: the
caller's binding when the operand was a `Var` nothing writes, the literal's type
when it was an immediate literal, or the fresh local the `let` bound. The ascent joins those records into the parameter's type
exactly as it joins a call site's operand. So a definition every caller passes
ints to still proves `%mul x x`, even when no call to it is left:

```lisp
(assert (compiles? "(defn f [x] (%mul x x)) (f 3)")
        "the grafted call still proves x in the definition")
```

### Operands and parameters

The graft binds each parameter to its operand in the order the operands are
written, so the evaluation order of the call is kept. An operand that is an
immediate literal, or a `Var` naming a binding nothing writes, replaces the
parameter directly in the body. Such a binding holds one value for the whole
body. The test is that nothing writes it, not that it is immutable, because a
`match` pattern's variable is never marked immutable. Every other operand is
bound by a `let` to a fresh local first. The direct replacement matters for the fold: a guard in the
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

A `cond` loses each clause whose test decides false. A clause whose test
decides true becomes the `cond`'s else, and the clauses after it go. An `and`
or an `or` stops at an operand that decides it: an `and` at a false operand, an
`or` at a true one. The operands after that one never run, so they go. An
operand before the last that decides the other way goes too, because its value
is never the result. So a literal operand the graft wrote in place of a
parameter never reaches an op its type test rules out:

```lisp
(assert (not (inf? nil)) "the and stops at (float? nil)")
(assert (compiles? "(defn f [x] (and (%int? x) (%bit-and x 1))) (f \"s\")")
        "the %bit-and after the false operand goes")
```

The fold asks no question about where the code came from. A branch the
programmer wrote is deleted exactly as a grafted one is, so a site inside a
guard that contradicts its operand's type is gone before the contract check,
rather than rejected by it:

```lisp
(assert (compiles? "(defn g [b] (if (%int? b) (%bit-and b 1) 0)) (g \"str\")")
        "no string reaches %bit-and, so the branch holding it goes")
```

### Certain divergence

A statement diverges when no path through it reaches the next statement: every
path raises, leaves through a `break`, or loops back through a `recur`. A
`block` diverges only when its tail diverges and no `break` inside it names
the block. Such a `break` leaves the block for the statement after it:

```lisp
(defn after-inner []
  "The value of the statement after a block its own break leaves."
  (block :outer (block :inner (break :inner 1)) :after))
(assert (= (after-inner) :after) "the inner block does not diverge")
```

The guard narrowing reads this: a statement after a guard whose branch
diverges knows the guard's condition was false ([type inference](typeinfer.md)).

The fold deletes no statement from a sequence. A raise does not end its
sequence, because an `:error` signal is resumable. A parent fiber that catches
the error can resume the child, and the child continues with the statement
after the raise ([fiber primitives](../signals/primitives.md)):

```lisp
(def resumed (fiber/new (fn [] (emit :error :stop) :done) |:error|))
(fiber/resume resumed)
(assert (= (fiber/resume resumed :again) :done) "the statement after the raise runs")
```

That leaves a site no type proves. In `(+ "a" 1)` the fold decides the guard on
`"a"` certain to raise, and the `%add` after it reads a string. So the contract
check skips every site after a diverging statement in the same sequence. Only a
resume reaches such a site, and it runs over the operand the guard rejected, as
the op after an undecided guard does. It lowers without a proof: the LIR reads
no operand as a type inference did not prove ([LIR](lir.md)).

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

A guard also reads through the `let` a graft introduces. `(not (integer? x))`
grafts to `%not` over a fresh local bound to the grafted `integer?`, and a
condition that reads a `let`-bound local proves what the value it was bound to
proves. Without that, the narrowing would stop at the first grafted wrapper: the
stdlib's `even?` guards `%rem x 2` with exactly this condition.

```lisp
(assert (compiles? "(defn h [x] (when (not (integer? x)) (error :x)) (%bit-and x 1))
                    (h 3) (h :k)")
        "the guard proves x an int through the grafted not and integer?")
```

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
until the WASM tier compares strings by content (#1268). `apply` is a macro, so
a comparison's general arm compares the operands past the first pair through
the spliced call `(< b ;more)`. That call does not count as recursion, so the
general arm grafts, and it leaves the one spliced call, which stays a call.
`not` takes `%not` for one operand, so a guard written with it narrows as one
written with `%not` does. The container wrappers
`push`, `put`, `add` and `del` dispatch on `(type-of coll)`, and `put` takes its
optional value through the rest list.

## Pinning tests

- `hir::typeinfer::tests::graft::*` — which calls graft. Each arithmetic and
  comparison wrapper over two proven operands leaves its op and no guard; an
  unproven pair grafts and keeps its runtime check. Three operands keep an
  arithmetic call and graft a comparison to one spliced call, and a spliced
  operand
  keeps the call. A user binding named `+` never reaches the stdlib body, a
  recursive function stays a call, a `(numeric!)` parameter keeps its floor in
  the graft, and a function the program defines grafts while its definition
  still proves its parameters. A call inside a function nothing calls stays a
  call.
- `hir::typeinfer::tests::fold::*` — what the fold decides: a guard over a
  proven operand, a `type-of` dispatch over a proven container in the program
  and in the stdlib, a certain raise that keeps what follows it, a resume that
  runs the statement after a raise, a guard that contradicts its operand, a
  deletion that narrows a join, a guard that proves through a grafted
  predicate and through a graft's `let`, a dead arm that binds a hoisted cell,
  and the error bit a dissolved body loses. It also pins a `cond` clause and
  the `and` and `or` operands a decided test removes, and a block its own
  `break` leaves, which proves nothing after it.
- `compiler::stdlib_cache::tests::fragment` — a stdlib restored from the disk
  cache grafts as a compiled one does.
- [the graft corpus test](../../tests/impl/graft.lisp) — every arity and every
  raise a grafted call keeps, on every tier.
- [the wrapper corpus test](../../tests/impl/wrapper-lowering.lisp) — each
  wrapper computes one value dissolved, grafted and called, over ints, floats,
  both zeros, NaN, infinities and the overflow edge.
- [the typed int ops](../../tests/impl/typed-int-ops.lisp) — the
  wrapper-spelled `fib` emits `AddInt` and `SubInt`.
