# Compile-Time Operations

<!-- audited: 2026-09-23 -->

The forms that act at compile time, and the `compile/*` API that reads the
compiler's model from running code.

Elle resolves every binding, analyzes captures, and infers signals before any
code runs (see [pipeline.md](pipeline.md)). It then exposes that work two ways:

- **Forms that *act* at compile time** — special forms and macros that are
  resolved during expansion/analysis and emit no runtime code (or emit
  annotations the later stages consume).
- **A `compile/*` API that *queries and transforms* the compile-time model from
  ordinary runtime code** — the reflective "superpower" the
  [README](../README.md) describes.

This page is the single index of all of them. Deep dives live in the linked
topic docs; this catalog is the map. Its examples run as one program under
`make doctest`.

## Timing at a glance

| Operation(s) | When | Emits runtime code? |
|---|---|---|
| `defmacro`, quasiquote, `macro?`, `expand-macro` | macro expansion (reader → analyzer) | expands to code |
| `(elle/epoch N)` | migration pass, before expansion | no (consumed) |
| `(unicode! …)` | analysis | no (declaration → `nil`; 0-arg query folds to the version array) |
| `silence`, `muffle`, `attune!` | signal inference | no (shapes the inferred signal) |
| `silent!`, `numeric!`, `immutable!` | post-inference checks | no (evaluate to `nil`) |
| `gate!` | macro expansion; the test runs at run time | yes (a runtime guard that raises `:gated`) |
| `when!` / `unless!` *(proposed)* | analysis | no (the excluded body is not compiled) |
| `emit` (signal keyword), `yield` | signal recorded at compile time | value emitted at runtime |
| `quote`, quasiquote, `environment` | analysis/expansion | yes (data / list construction) |
| `%`-intrinsics | proven at compile time, lowered | yes (one VM instruction; storing ops a native funnel call) |
| `compile/*` | runtime, over the compile model | n/a (reflective) |
| `eval`, `read`, `read-all` | runtime (listed for contrast) | yes |

## Naming conventions

Three prefixes/suffixes signal *when* an operation acts:

- **`name!` — a compile-time-only special form.** It produces no runtime code;
  the analyzer checks it and the form evaluates to `nil`. The bang is the
  project's marker for "this is resolved by the compiler, not run." Existing
  members: `silent!`, `numeric!`, `immutable!`, `attune!`, `unicode!`. Two
  members bend the rule. `(unicode!)` with no arguments is a query, not an
  assertion, and folds to the selected Unicode version array. `gate!` is a
  prelude macro whose test runs at run time.
- **`%name` — an intrinsic.** Compile-time proven against its operand contract,
  then lowered to a single VM instruction (storing ops to the native funnel
  call) — no runtime validation, no signal emission, no rest-arg allocation;
  an unproven call is a compile error. See [intrinsics.md](intrinsics.md).
- **`compile/name` — a reflective op over the compile-time model**, callable
  from normal runtime code. See [analysis/portrait.md](analysis/portrait.md).

## Macro expansion

Macros are defined with `defmacro` (alias `define-macro`) and expand **between
the reader and the analyzer** — arguments arrive quoted, the body is compiled and
run on the VM, and the result is converted back to syntax. Expansion is hygienic
via sets-of-scopes. Two expand-time introspection forms exist: `(macro? name)`
answers whether `name` names a macro, and `(expand-macro 'form)` expands a form
written with the `'` reader prefix and hands the expansion back as data.

```lisp
(defmacro inc-by-one (x) `(+ ,x 1))
(assert (macro? inc-by-one))
(assert (not (macro? println)))
(assert (= (inc-by-one 41) 42))
(def expansion (expand-macro '(inc-by-one 3)))
(assert (= (length expansion) 3))   # (+ 3 1)
(assert (= (get expansion 1) 3))
```

Full hygiene semantics, scope sets, and the `datum->syntax` escape hatch:
[macros.md](macros.md).

## Signal-shaping forms

Signals are inferred at compile time and flow up from callee to caller (see
[signals/](signals/)). Several forms constrain or annotate that inference:

| Form | Effect |
|---|---|
| `(silence)` | this function's ceiling is silent: no signal at all, `:error` included |
| `(silence f)` | a closure passed for parameter `f` must be silent |
| `(attune! :io)`, `(attune! \|:io :yield\|)` | the most this function may emit |
| `(muffle :error)` | remove these signals from the inferred signal; nothing stops them at run time |
| `(emit :io)`, `(emit :yield value)` | emit a signal; the keyword is recorded at compile time, the value is emitted at run time |
| `(yield value)` | a macro for `(emit :yield value)` |

`silence`, `muffle`, `attune!`, and `emit` are **special forms** analyzed during
signal inference. `squelch` and the runtime `attune` are **runtime wrappers** that
intercept a closure's signals after the fact while also narrowing the inferred
signal at compile time — see [signals/](signals/) for the full model and the
exact two-argument shape of `squelch`.

```lisp
(defn compiles? [src]
  (first (protect (compile/whole-module src "<doc>"))))

(assert (compiles? "(defn pick [b x y] (silence) (if b x y))"))
(assert (not (compiles? "(defn loud [] (silence) (emit :yield 1))")))
(assert (not (compiles? "(defn add [x y] (attune! :yield) (error :no))")))
```

## Compile-time assertions

These evaluate to `nil` and emit no code; they assert a property the analyzer
verifies, failing compilation if violated:

| Form | Asserts |
|---|---|
| `(silent!)` | this function emits no signals |
| `(numeric!)` | every parameter is a number, and the body is GPU-eligible: no call, no closure, no heap value, no emit |
| `(immutable! name)` | the binding `name` is never assigned |

```lisp
(assert (compiles? "(defn one [] (silent!) 1)"))
(assert (not (compiles? "(defn gen [] (silent!) (yield 1))")))
(assert (compiles? "(defn square [x] (numeric!) (%mul x x))"))
(assert (not (compiles? "(defn k [] (def @z 1) (immutable! z) (assign z 2) z)")))
```

## Conditional compilation

A general compile-time gate, in two variants, designed for the test runner but
useful language-wide (Elle's `#[cfg]`). Specified in
[test-runner.md](test-runner.md); recorded here so the compile-time catalog
stays the single home.

The loud variant exists today as a prelude macro. `(gate! COND "reason" body…)`
runs `body` when `COND` is truthy, and otherwise raises an error whose `:error`
is `:gated` and whose `:reason` is the string, so a harness can account for the
skip. `COND` runs at run time; `(backend? :jit)` is the canonical test.

```lisp
(assert (= (gate! true "always open" 1) 1))
(def [ran? gated] (protect (gate! false "closed on purpose" 1)))
(assert (not ran?))
(assert (= (get gated :error) :gated))
(assert (= (get gated :reason) "closed on purpose"))
```

*Proposed, not implemented:* the silent `(when! COND body…)` and
`(unless! COND body…)`, which would leave an excluded body uncompiled, and the
compile-time predicate `(feature? :ffi)`. A companion `%assert` intrinsic
(carrying the asserted predicate's syntax, and elidable when provably true or in
an assertions-disabled build) is proposed alongside; see
[test-runner.md](test-runner.md).

## Epoch selection

`(elle/epoch N)` as the first form in a file selects the syntax epoch.
`(elle/epoch)` with no arguments returns the current epoch number.

```lisp
(assert (= (elle/epoch) 12))
```

The epoch migration pass runs **after parsing, before macro expansion**, applying
backward-compatible syntax rewrites; the declaration form itself is consumed.
Migration rule types and the current epoch: [epochs.md](epochs.md).

## Unicode generation selection

`(unicode! N [MIN [PATCH]])` declares the Unicode generation this source
assumes. `(unicode!)` with no arguments folds to the selected version.

```lisp
(def version (unicode!))
(assert (= (length version) 3))   # [major minor patch], [17 0 0] by default
```

Where epochs version the *syntax*, `unicode!` versions the *string semantics*:
`length`, `get`, and `slice` count UAX #29 grapheme clusters, and each build
vendors one or more table generations. The generation is locked per VM before
construction, from three surfaces that must agree: the declaration in the main
file, the `--unicode=` CLI flag, and the embedding constructor. Default: the
newest vendored generation. After the lock, every `(unicode! …)` in the
program is an assertion against the locked generation — a conflict or a
non-vendored request is a compile error naming the versions involved. The
declaration selects real behavior, not just a check: under `(unicode! 16)`
strings segment with the Unicode 16.0 tables. Details and examples:
[strings.md](strings.md).

## Quoting and scope reification

| Form | Result |
|---|---|
| `(quote form)`, `'form` | the form, unevaluated, as data |
| `` `form ``, `,x`, `,;xs` | quasiquote, unquote and unquote-splicing, expanded to list construction |
| `(environment)` | the current lexical scope as a struct keyed by quoted symbols; pairs with `eval` |

`quote` is recognized by the analyzer; quasiquote is expanded to runtime list
construction; `environment` desugars to a struct of the in-scope bindings.

```lisp
(let [a 1
      b 2]
  (assert (= (get (environment) 'b) 2))
  (assert (= (eval '(+ a b) (environment)) 3)))
```

## Reflecting the compile-time model at runtime — `compile/*`

`(compile/analyze source [opts])` runs the reader → expander → analyzer at
runtime and returns an opaque **analysis handle**; every other operation
queries or transforms that handle. This is how user code (and agents) read the
same semantic model the compiler builds. Full guide:
[analysis/portrait.md](analysis/portrait.md); agent usage patterns:
[analysis/agent-reasoning.md](analysis/agent-reasoning.md).

```lisp
(def analysis (compile/analyze "(defn pick [b x y] (if b x y))" {:file "pick.lisp"}))
(assert (get (compile/signal analysis :pick) :silent))
```

**Queries (pure, return structured data):**

| Operation | Returns |
|---|---|
| `compile/diagnostics analysis` | warnings/errors `{:severity :code …}` |
| `compile/symbols analysis` | all symbols with metadata |
| `compile/signal analysis :fn` | inferred signal of a function |
| `compile/query-signal analysis :query` | functions matching `:silent`/`:io`/`:yields`/`:jit-eligible`/… |
| `compile/bindings analysis` | all bindings (scope, mutability, capture) |
| `compile/binding analysis :name` | one binding's detail |
| `compile/captures analysis :fn` | what a function captures, and how |
| `compile/captured-by analysis :name` | functions capturing a binding |
| `compile/callers analysis :fn` | call-graph in-edges |
| `compile/callees analysis :fn` | call-graph out-edges |
| `compile/call-graph analysis` | `{:nodes :roots :leaves}` |
| `compile/exports analysis` | the module surface: `{:constructor :exports}`, or nil |
| `compile/parallelize analysis [:f1 :f2 …]` | whether functions may run in parallel |
| `compile/primitives` | metadata for all Rust-defined primitives |

**Transforms (return new source text + a fresh handle):**

| Operation | Effect |
|---|---|
| `compile/rename analysis :old :new` | binding-aware rename |
| `compile/extract analysis {:from :fn :lines [s e] :name :new}` | extract a range into a new function |
| `compile/add-handler analysis :fn :signal` | wrap call sites with signal handling |

**Execution:** `(compile/run-on tier closure & args)` runs the closure on one
tier: `:bytecode`, `:jit`, `:mlir-cpu` (built with MLIR) or `:wasm` (built with
WASM). A tier that declines the closure raises `:tier-rejected`.

```lisp
(assert (= (compile/run-on :bytecode (fn [a b] (+ a b)) 3 4) 7))
```

## Intrinsics

`%`-prefixed operations, compile-time prove-or-reject: the compiler proves each
call's operands against the op's soundness contract, and proven operands lower
to one VM instruction (storing ops to the native funnel call); an unproven call
is a compile error. In value position an intrinsic is its registered `NativeFn`,
which validates at runtime when called dynamically. The complete list and the
trade-offs: [intrinsics.md](intrinsics.md).

## Runtime reflection (for contrast — *not* compile-time)

These run the pipeline at runtime and are listed only to disambiguate them from
the compile-time forms above:

| Form | Effect |
|---|---|
| `(eval expr [env])` | compile and execute a quoted datum at runtime |
| `(read string)` | parse the first form from a string |
| `(read-all string)` | parse every form from a string |

## See also

- [macros.md](macros.md) — macro system, hygiene, `datum->syntax`
- [intrinsics.md](intrinsics.md) — the `%`-intrinsic reference
- [epochs.md](epochs.md) — epoch declaration and migration rules
- [signals/](signals/) — signal inference, `silence`/`squelch`, capabilities
- [analysis/portrait.md](analysis/portrait.md) — the `compile/*` API in depth
- [analysis/agent-reasoning.md](analysis/agent-reasoning.md) — how agents query the model
- [test-runner.md](test-runner.md) — where `when!`/`unless!`/`gate!`/`%assert` are specified
- [pipeline.md](pipeline.md) — the full compile pipeline these operations hook into
