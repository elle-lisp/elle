# Dissolution: whose body the loop splices

<!-- audited: 2026-09-28 -->

How a named function's body reaches a fused loop, and what the spliced body
keeps: its counters and its declarations.

The [dissolution](../dissolution.md) document owns the pass and the call-site
lambda literal every path here is read against.

## Named same-unit functions

A HOF's function argument need not be a call-site lambda literal. When it is a
`Var` referencing a binding **in the same compile unit** whose initializer is a
qualifying lambda — a top-level `(defn f …)` (which desugars to `(def f (fn …))`)
or any `let`/`def`-bound `(fn …)` — the function's body is inlined into the fused
loop, exactly as a literal's would be. The map from a binding to its fragment is
collected by the same walk `prune.rs` uses for init keywords (over
`Let`/`Letrec`/`Define` bindings), restricted to immutable, singly-bound lambdas
of the op's arity whose body closes.

The one structural difference from a literal is **who owns the body**. A literal
is consumed — moved out of the call — so its parameters and node ids belong
uniquely to the splice. A **named** function *persists*: it stays bound and may
be called elsewhere as a first-class value, so its body cannot be moved. It is
collected as an `HirFragment` — a body closed over its own bindings
([impl/hir.md](../hir.md)) — and each
call site **grafts** a copy: the parameters and any `let` bindings are re-minted
in this unit's arena, the free globals resolve by name, and every node is rebuilt
with a fresh `HirId`. The fragment decides which bodies qualify, so a body that
binds through a form the close cannot model declines here: the HOF stays a plain
call and the definition's own bindings are never duplicated.

Resolving a free global by name gives back the binding it had: `bind_primitives`
binds each primitive name once, so name and binding stand in one-to-one
correspondence within a unit. The other two cases cannot mis-resolve either. A
free variable that is an enclosing runtime local never reaches a fragment — the
close refuses it. One that is a module name of *this* unit is recorded, but it
is not `is_primitive` here, so the graft finds nothing and declines. That last
case is what separates this section from the next.

## Cross-unit named functions

A named function need not live in the unit that calls it. A **stdlib** `defn` —
`inc`, `dec`, and any lambda whose body closes — is defined in the `<stdlib>`
compile unit, so a later user unit that writes `(map inc xs)` has no body for
`inc` in its own tree and its by-binding map holds nothing for it. The
by-name registry carries the body across the compile-unit boundary, mirroring
the dispatch-wrapper registry
(`monomorphize.rs`): a per-instance registry keyed by function **name**
(`SymbolId`, stable across arenas where a `Binding` is not) is populated as each
unit compiles — the `<stdlib>` compile records `inc` — and consulted by every later
unit, gated on the callee being `is_primitive` (a `bind_primitives` stdlib export;
a user redefinition shadows it with a non-primitive binding and is left alone).

The registry entry is the same `HirFragment` the same-unit map holds. Nothing of
the defining unit is consulted at the call site: the fragment's own table carries
the metadata of every binding its body introduces, and the graft's `Global`
resolution — the arithmetic op in `(+ x 1)`, say — is the by-name re-resolution
the boundary needs. If a free global does not resolve in the consuming unit, the
graft **declines** (the HOF stays a plain call) — correct-by-construction, never
a mis-resolved reference. A fragment is plain data, so the registry also crosses
the stdlib disk cache whole: a cache hit inlines the set a stdlib compile
records, not a subset of it ([src/compiler/stdlib_cache.rs](../../../src/compiler/stdlib_cache.rs)).

One gate fills both maps, and what separates the two paths is **where a
fragment's free globals resolve**. When the stdlib compiles, its own exports are
not yet `is_primitive` — `+` is a file-scope (`is_file_scope`) letrec sibling —
so `inc`'s fragment records `+` as a global that the defining unit cannot itself
resolve, and a `(map inc …)` written inside stdlib declines. In a later unit `+`
is a `bind_primitives` export, so the same fragment resolves and grafts. A stdlib
`defn` referencing only other globals therefore inlines into user code, while a
function reading an enclosing runtime local never becomes a fragment at all.

Everything else in the gate is identical — fixed arity, no rest parameter,
unmutated parameters, and the composition reorder requirement, read from the
body signal the fragment records.


## The scaffold's own counters advance by opcode

Every counter the scaffold owns — the walk's induction variable, a `mapcat`'s inner
one, a `find-index`'s survivor count, a `count`'s tally — advances through the raw
`%add` intrinsic (`Build::advance`), never through the stdlib `+`. That is what the
walks this loop stands in for do ([src/prelude.lisp](../../../src/prelude.lisp)'s `each` macro), and the reason
is the pass's own thesis: `+` is a variadic function that collects a rest-list and
folds it with a `letrec` walker, so reaching it once per element would mint that
list, the walker's closure and its cell — re-creating per element exactly the closure
fusion exists to dissolve, and swamping the intermediate collection the fusion
removed. The comparison `(< i len)` stays a call: it lowers to an opcode already, so
it mints nothing.

The intrinsic carries a prove-or-reject operand contract ([docs/intrinsics.md](../../intrinsics.md)),
discharged here by the counter's own type rather than by any declaration: the
scaffold seeds each counter at the literal `0` and advances it by this one site, so
nothing but a number ever reaches it. This is the same proof `each`'s
`(def @idx 0)` / `(%add idx 1)` pair rests on.

## Raw `%`-intrinsic bodies — the declaration travels with the binding

A `%`-intrinsic in call position must discharge its operand contract from the
inferred operand types ([docs/intrinsics.md](../../intrinsics.md) § "The contract: prove or reject"),
and for a numeric kernel written over a parameter — `(fn [x] (numeric!) (%mul x
x))` — the fact that discharges it is the `(numeric!)` declaration, which floors
every parameter of the function at Number. Fusion dissolves the function, so a
declaration scoped to the *lambda node* would vanish with it: the spliced
`(%mul x x)` now reads the loop's `(get coll i)` element, whose type is not
tracked, and the site would fail to prove. That would turn a compiling program
into a compile error — which no rewrite may do.

So the declaration is recorded where it is *about*. `(numeric!)` floors the
function's **parameter bindings** (`BindingInner::declared_numeric`), and
inference applies that floor wherever such a binding gets its type: as a
lambda parameter, as a parameter joined from its call sites, and as the
`let`-bound loop local the splice turns the parameter into. Fusion carries the
flag with the parameter — a call-site lambda literal is *moved*, so its
(already-flagged) parameter binding travels as-is; a fragment holds the whole
`BindingInner` per parameter, so the graft mints one carrying the flag along
with every other binding fact. The fact that discharged the intrinsic
inside the function is the same fact that discharges it inside the loop, so
fusion leaves *whether the program compiles* unchanged.

The gate is therefore the declaration, not the op: a body containing a
call-position `%`-intrinsic is admitted **only** under `(numeric!)`. Without the
declaration there is nothing to carry, so the body declines and the HOF stays a
plain call — even where the intrinsic's operands are all literals and would prove
on their own. This is the shape the fusion most wants: a raw-intrinsic kernel over
a proven array is the numeric loop a SIMD/GPU realization tier consumes, with no
per-element closure, no dispatch, and no wrapper call between the elements and the
opcode.

Every other proof an intrinsic body may rest on is structural and survives the
splice unchanged — a literal operand, a diverging type guard inside the body, a
global's inferred type. Only the parameter floor is scoped to the function, and
only it needs carrying.

`(numeric!)` carries a second, independent assertion: that the function's body is
GPU-eligible, checked when the lambda lowers as a function
([src/lir/lower/lambda/expr.rs](../../../src/lir/lower/lambda/expr.rs)). A fused lambda never lowers as a function, so
that half of the declaration has nothing left to hold — true of every fused
`(numeric!)` lambda, intrinsic body or not. The type floor is the half fusion
carries.
