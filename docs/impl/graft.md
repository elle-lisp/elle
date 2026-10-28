# Grafting a call

<!-- audited: 2026-10-28 -->

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
- The call sits in live code, or the selected body holds no op with a proof
  obligation (see "Code nothing reaches").

A call that fails any of these stays the call it was. Nothing else is asked:
there is no declaration and no list of names. A stdlib function and a function
the program defines graft alike.

### Code nothing reaches

Code outside every lambda is live. A lambda written in live code is live,
unless it initializes a binding: then it is live when live code names that
binding, as a callee or as a value. A file binds each top-level expression
before its last to a name nothing reads, so a lambda such an expression builds
is not live, whatever its body names.

Such a function's parameters take no type from any call, so they prove nothing
([type inference](typeinfer.md)). In a lambda nothing live reaches, the graft
therefore replaces only a call whose selected body holds no `%`-op with a
proof obligation: a predicate, a one-operand `not`, an equality. A body with
an op to prove, grafted there, would put that op over operands that prove
nothing, while the same op in the callee's own definition proves against the
callers that do exist. A body with nothing to prove adds no site the contract
could reject, and grafting it is what lets a guard written with a predicate of
this unit narrow there. The stdlib compiled as one unit with a program after
its export closure proves the `%rem` in `even?` only that way.

Below, `go` calls `helper` and nothing calls `go`, so the program compiles only
while that call stays a call. Nothing names `low-bit` either: the keyword after
it is the file's value, so no live code reaches it. Its guard still proves the
`%bit-and` through the grafted `int-only?`:

```lisp
(assert (compiles? "(defn plain [n]
                      (letrec [helper (fn [x] (%sub x 1))
                               go (fn [m] (helper m))]
                        (helper n)))
                    (plain 5)")
        "the call in go stays a call, and helper's x is proven by (helper n)")
(assert (compiles? "(defn int-only? [x] (match (type-of x) :integer true _ false))
                    (defn low-bit [x]
                      (when (%not (int-only? x)) (error :x))
                      (%bit-and x 1))
                    :done")
        "the guard narrows through the grafted int-only?, and nothing reaches low-bit")
```

### Where the body comes from

A function this unit defines is in the tree being compiled. The graft closes its
initializer's body into a fragment before it grafts any call.

A function from another unit is reached through its value. When a unit is
analyzed, each binding the earlier units export maps to its compile-time value,
and for a stdlib function that value is a closure. A closure's code payload
carries the fragment of its own body as bytes, closed before functionalization
in the unit that defined it and encoded when the lambda lowers
([code objects](region/template.md)). The graft reads the fragment off the
value the binding holds. It decodes a payload's bytes once per compile, into a
memo keyed by the payload, so a unit that calls `+` a hundred times decodes
`+` once. Bytes that do not decode are a body the payload does not carry, and
the call stays a call. A user binding that shadows `+` holds a different
value, so it can never reach `+`'s body.

Such a fragment names its free globals by name. The graft resolves each one
among the bindings the runtime supplies to this unit: the primitives and the
stdlib's exports. A global that does not resolve keeps the call a call, so the
stdlib exports every helper a wrapper's body names, such as `dynamic-lt` and
`check-comparable`.

The stdlib disk cache stores each payload's fragment with the payload
([the stdlib cache](stdlib-cache.md)), and a boot image maps it with the
payload ([booting from an image](image/boot.md)). So a call into the stdlib
grafts the same body under a source boot, a cache hit and an image boot.

### A grafted call still proves the definition

A function this unit defines keeps its definition, and its parameters take their
types from the calls this unit makes to it ([type inference](typeinfer.md)).
Grafting a call removes it from the tree, so the graft records what the call
would have contributed. For each parameter it notes the operand it bound: the
caller's binding when the operand was a `Var` nothing writes, the literal's type
when it was an immediate literal, or the fresh local the `let` bound. The ascent
joins those records into the parameter's type exactly as it joins a call site's
operand. So a definition every caller passes ints to still proves `%mul x x`,
even when no call to it is left:

```lisp
(assert (compiles? "(defn f [x] (%mul x x)) (f 3)")
        "the grafted call still proves x in the definition")
```

### The operands

Which operand is written in place of its parameter, which a `let` binds first,
and how the known operand count decides a rest-list dispatch are
[the operands](graft/operands.md).

## The fold

After the inference fixpoint the fold rewrites what the recorded types decide.
It then hands the tree back to inference, and the two alternate until a fold
changes nothing. The deletions can narrow a join, and a later guard or op may
prove only against the narrower type. The contract check, the signal narrowing
and the signal re-propagation run on the tree the last fold left
([type inference](typeinfer.md)).

### A record never widens

A fold deletes a grafted call that sits in a branch or an arm no value reaches,
and the `let` that bound its operand goes with it. The round of inference after
that fold has no node left to read the operand's type from. So each round reads
a record as the meet of what the tree gives it and what the round before
settled on, and a record the tree no longer holds keeps its last reading.
Without the meet, the parameter would widen past a type the fold had already
deleted guards for.

Below, `each` expands to one arm for each kind of sequence, and the fold
deletes every arm but the array's. Every arm passed `g` a float, so the same
fold deletes the number guard of the `*` in `h`. The deleted arms keep their
float, so the `%mul` that `*` leaves in `h` still proves:

```lisp
(assert (compiles? "(defn h [g] (* 0.7 g))
                    (each s in [1 2] (h (+ 0.5 0.5)))")
        "the deleted arms still record a float for g")
```

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
arm is decided becomes that arm's body. A `when` guard that decides false
becomes the `nil` of its untaken side, and a sequence drops such a statement:
an immediate literal before the tail has no effect and no reader. A sequence
left with its tail alone becomes the tail, so a grafted wrapper whose guards
both go is its op and nothing else.

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

The fold deletes no statement that runs from a sequence, only a literal a
decided guard left behind. A raise does not end its sequence, because an
`:error` signal is resumable. A parent fiber that catches
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
function does not change, so `(silent!)` accepts and refuses exactly the
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
- A raise in a grafted body is a raise in the caller's frame. A restart
  answers it there and the grafted body runs on, exactly as a restart answers
  a raise inside a callee whose call stayed a call
  ([fiber primitives](../signals/primitives.md)).
- A grafted body from this unit keeps the source locations it was written
  with. So an error it raises names the raising form, as it did while the call
  stayed a call ([the VM](vm.md)).
- A grafted body from another unit takes the location of the call it replaced.
  Its own spans name files another process may have interned, as a fragment
  read from the stdlib cache does. So an error a grafted stdlib wrapper raises
  names the program's call.
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

## Turning the graft off

An implementation test whose subject is a call needs the call to stay one: a
test of the region a call hands its callee, or of the frame a callee owns. Such
a test turns the graft off. It does not write a callee the graft declines, such
as one with an unused `&opt` parameter, because that relies on a limit of the
graft and not on a rule of the language ([spec](../spec.md)).

An Elle file names `graft = false` in its rig sidecar
([the rig](../../rig/overview.md)). A Rust test sets `GraftPolicy::Off` on the
`CompileCtx` it compiles through. No flag of `elle` and no `vm/config` key
reaches the setting.

The setting holds for each unit an instance compiles after its boot: the
program, the files it imports, and a `compile/whole-module` source. The boot
compiled the stdlib under the graft, so the stdlib's own bodies keep their
grafts, while a call from the program into the stdlib stays a call. The fold
still runs, and deletes what the types decide around the calls that stay.

## Pinning tests

- `hir::typeinfer::tests::graft::*` — which calls graft. Each arithmetic and
  comparison wrapper over two proven operands leaves its op and no guard; an
  unproven pair grafts and keeps its runtime check. A user binding named `+`
  never reaches the stdlib body, a recursive function stays a call, and a
  function the program defines grafts while its definition still proves its
  parameters. Under `GraftPolicy::Off` every call stays a call, to a function
  the program defines and to the stdlib, and the stdlib keeps its own grafts. A body the program defines keeps its own lines where
  it grafts, and a stdlib body takes the line of the call.
- `hir::typeinfer::tests::live::*` — which lambdas the graft reaches. In a
  function nothing calls, a call whose body holds an op to prove stays a call,
  a predicate of this unit grafts and its guard narrows, and so does a
  one-operand negation of this unit. The stdlib source followed by a program
  compiles as one unit.
- [the error-location tests](../../tests/integration/error_reporting.rs) — an
  error raised in a grafted body of the program is reported at the raising
  form.
- `hir::typeinfer::tests::fold::*` — what the fold decides: a guard over a
  proven operand, a `type-of` dispatch over a proven container in the program
  and in the stdlib, a certain raise that keeps what follows it, a resume that
  runs the statement after a raise, a guard that contradicts its operand, a
  deletion that narrows a join, a guard that proves through a grafted
  predicate and through a graft's `let`, a dead arm that binds a hoisted cell,
  and the error bit a dissolved body loses. It also pins a `cond` clause and
  the `and` and `or` operands a decided test removes, a block its own
  `break` leaves, which proves nothing after it, and the literal statement a
  decided guard leaves, which goes.
- `hir::typeinfer::tests::record::*` — a grafted call in an arm the fold
  deletes keeps the type it recorded, so the guard the same fold deleted stays
  proven.
- [the each test](../../tests/lang/each.lisp), a language test — a function
  that a loop over an array or an `@array` calls proves its wrapper's operands,
  at file scope and inside a function.
- `hir::typeinfer::tests::graft::a_payloads_fragment_decodes_once_per_compile`
  — two lookups of one stdlib function in one compile hand out one fragment.
- `value::closure::tests::payload` — the payload record is exactly its
  fields, the fragment among them.
- `lir::emit::tests::a_nested_lambdas_payload_carries_its_fragment` — the
  bytes the lowerer put on the function's head reach its payload.
- `compiler::stdlib_cache::tests::fragment` — a stdlib restored from the disk
  cache grafts as a compiled one does, and the stored form carries the bytes.
- `image::boot::tests::a_call_to_a_stdlib_function_grafts_under_either_boot`
  and `..::the_two_boots_emit_one_bytecode_for_a_user_file` — a hydrated
  closure grafts as a compiled one does, and a user file compiles to one
  bytecode under both boots.
- [the graft corpus test](../../tests/impl/graft.lisp) — a branch no value
  reaches goes without the binding the rest of the program reads, a deletion
  narrows a join, and a guard narrows through the wrappers it is written with.
- [the wrapper corpus test](../../tests/impl/wrapper-lowering.lisp) — each
  wrapper over two proven numbers leaves no `Emit` in the disassembly, and over
  unproven operands it keeps its guards.
- [the graft-off corpus test](../../tests/impl/graft-off.lisp) — under
  `graft = false`, a call to a function the file defines and a call to `+`
  each stay a call in the disassembly.
- [the rig's sidecar tests](../../rig/tests/sidecar.rs) — the rig reads
  `graft`, prints it back, and refuses a value that is not a boolean.
- [the operators test](../../tests/lang/operators.lisp), a language test —
  each wrapper answers at every arity and raises what it raises. It answers
  alike over literal operands, over operands a function receives, and called
  as a value, over ints, floats, both zeros, NaN, infinities and the overflow
  edge.
- [the typed int ops](../../tests/impl/typed-int-ops.lisp) — the
  wrapper-spelled `fib` emits `AddInt` and `SubInt`.
