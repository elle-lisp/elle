# Compile-Time Operations

Elle does deep work before any code runs — full binding resolution, capture
analysis, and signal inference all happen at compile time (see
[`pipeline.md`](pipeline.md)). It then exposes that work two ways:

- **Forms that *act* at compile time** — special forms and macros that are
  resolved during expansion/analysis and emit no runtime code (or emit
  annotations the later stages consume).
- **A `compile/*` API that *queries and transforms* the compile-time model from
  ordinary runtime code** — the reflective "superpower" the README describes.

This page is the single index of all of them. Deep dives and runnable, asserted
examples live in the linked topic docs; this catalog is the map.

> Code blocks here are untagged signatures, not executed examples — the runnable
> assertions are in the linked docs. (Every `docs/*.md` is a program run by
> `make doctest`; see [`README.md`](README.md).)

## Timing at a glance

| Operation(s) | When | Emits runtime code? |
|---|---|---|
| `defmacro`, quasiquote, `macro?`, `expand-macro` | macro expansion (reader → analyzer) | expands to code |
| `(elle/epoch N)` | migration pass, before expansion | no (consumed) |
| `silence`, `muffle`, `attune!` | signal inference | no (shapes the inferred signal) |
| `silent!`, `numeric!`, `immutable!` | post-inference checks | no (evaluate to `nil`) |
| `when!` / `unless!` / `gate!` *(proposed)* | analysis | silent: elided · loud: emits `:gated` |
| `emit` (signal keyword), `yield` | signal recorded at compile time | value emitted at runtime |
| `quote`, quasiquote, `environment` | analysis/expansion | yes (data / list construction) |
| `%`-intrinsics | lowering | yes (one VM instruction) |
| `compile/*` | runtime, over the compile model | n/a (reflective) |
| `eval`, `read`, `read-all` | runtime (listed for contrast) | yes |

## Naming conventions

Three prefixes/suffixes signal *when* an operation acts:

- **`name!` — a compile-time-only special form.** It produces no runtime code;
  the analyzer checks it and the form evaluates to `nil`. The bang is the
  project's marker for "this is resolved by the compiler, not run." Existing
  members: `silent!`, `numeric!`, `immutable!`, `attune!`.
- **`%name` — an intrinsic.** Lowers straight to a single VM instruction with no
  validation, no signal emission, no rest-arg allocation. See
  [`intrinsics.md`](intrinsics.md).
- **`compile/name` — a reflective op over the compile-time model**, callable
  from normal runtime code. See [`analysis/portrait.md`](analysis/portrait.md).

## Macro expansion

Macros are defined with `defmacro` (alias `define-macro`) and expand **between
the reader and the analyzer** — arguments arrive quoted, the body is compiled and
run on the VM, and the result is converted back to syntax. Expansion is hygienic
via sets-of-scopes. Two expand-time introspection helpers exist:

```
(defmacro name (param…) body…)     # positional params; body returns syntax
(macro? symbol)                    # true at expansion time if symbol names a macro
(expand-macro '(form …))           # expand a quoted form, return syntax
```

Full hygiene semantics, scope sets, and the `datum->syntax` escape hatch:
[`macros.md`](macros.md).

## Signal-shaping forms

Signals are inferred at compile time and flow up from callee to caller (see
[`signals/`](signals/)). Several forms constrain or annotate that inference:

```
(silence)                # declare this function's signal ceiling = Silent
(attune! :io)            # ceiling = exactly these signals (generalizes silence)
(attune! |:io :yield|)
(muffle :error)          # absorb these bits — allowed internally, hidden externally
(emit :io)               # emit a signal; the keyword is recorded at compile time,
(emit :yield value)      #   the value is emitted at runtime
(yield value)            # macro → (emit :yield value)
```

`silence`, `muffle`, `attune!`, and `emit` are **special forms** analyzed during
signal inference. `squelch` and the runtime `attune` are **runtime wrappers** that
intercept a closure's signals after the fact while also narrowing the inferred
signal at compile time — see [`signals/`](signals/) for the full model and the
exact two-argument shape of `squelch`.

## Compile-time assertions

These evaluate to `nil` and emit no code; they assert a property the analyzer
verifies, failing compilation if violated:

```
(silent!)                # assert: this function emits no signals
(numeric!)               # assert: all parameters are numeric (GPU eligibility)
(immutable! binding)     # assert: binding is never assigned
```

## Conditional compilation *(proposed — not yet implemented)*

A general compile-time gate, in two variants, designed for the test runner but
useful language-wide (Elle's `#[cfg]`). Specified in
[`test-runner.md`](test-runner.md); recorded here so the compile-time catalog
stays the single home.

```
(when! COND body…)              # silent: if COND (compile-time) is false, body is
(unless! COND body…)            #   not compiled — no code, no trace
(gate! COND "reason" body…)     # loud: an unmet COND emits (emit :gated {:reason …})
                                #   so a harness can account for the skip
```

`COND` draws on compile-time predicates (also proposed): `(backend? :jit)`,
`(feature? :ffi)`, and friends. When `COND` is compile-time-constant the dead
branch is never compiled; when it isn't (e.g. a runtime library check) `gate!`
lowers to a runtime guard that emits `:gated`. A companion `%assert` intrinsic
(carrying the asserted predicate's syntax, and elidable when provably true or in
an assertions-disabled build) is proposed alongside; see
[`test-runner.md`](test-runner.md).

## Epoch selection

```
(elle/epoch N)           # first form in a file; selects the syntax epoch
(elle/epoch)             # with no args, returns the current epoch number
```

The epoch migration pass runs **after parsing, before macro expansion**, applying
backward-compatible syntax rewrites; the declaration form itself is consumed.
Migration rule types and the current epoch: [`epochs.md`](epochs.md).

## Quoting and scope reification

```
(quote form)   /  'form    # unevaluated form as data (HirKind::Quote)
`form  ,x  ,@xs            # quasiquote / unquote / unquote-splicing → list construction
(environment)             # reify the current lexical scope as a struct (pairs with eval)
```

`quote` is recognized by the analyzer; quasiquote is expanded to runtime list
construction; `environment` desugars to a struct of the in-scope bindings.

## Reflecting the compile-time model at runtime — `compile/*`

`compile/analyze` runs the reader → expander → analyzer at runtime and returns an
opaque **analysis handle**; every other operation queries or transforms that
handle. This is how user code (and agents) read the same semantic model the
compiler builds. Full guide: [`analysis/portrait.md`](analysis/portrait.md);
agent usage patterns: [`analysis/agent-reasoning.md`](analysis/agent-reasoning.md).

```
(compile/analyze source [opts])   # → analysis handle (parse + expand + analyze)
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
| `compile/parallelize analysis [:f1 :f2 …]` | whether functions may run in parallel |
| `compile/primitives` | metadata for all Rust-defined primitives |

**Transforms (return new source text + a fresh handle):**

| Operation | Effect |
|---|---|
| `compile/rename analysis :old :new` | binding-aware rename |
| `compile/extract analysis {:from :fn :lines [s e] :name :new}` | extract a range into a new function |
| `compile/add-handler analysis :fn :signal` | wrap call sites with signal handling |

**Execution:**

```
(compile/run-on :tier closure & args)   # force a tier (:bytecode :jit :mlir-cpu :wasm);
                                         #   errors :tier-rejected if the backend declines
```

## Intrinsics

`%`-prefixed operations that lower to a single VM instruction (arithmetic,
comparison, logic, conversion, data access, …). Default mode inlines them with no
checks; `--checked-intrinsics` routes them through validating `NativeFn`s. The
complete list and the trade-offs: [`intrinsics.md`](intrinsics.md).

## Runtime reflection (for contrast — *not* compile-time)

These run the pipeline at runtime and are listed only to disambiguate them from
the compile-time forms above:

```
(eval expr [env])    # compile and execute at runtime (signal: Yields)
(read string)        # parse the first form from a string, at runtime
(read-all string)    # parse all forms from a string, at runtime
```

## See also

- [`macros.md`](macros.md) — macro system, hygiene, `datum->syntax`
- [`intrinsics.md`](intrinsics.md) — the `%`-intrinsic reference
- [`epochs.md`](epochs.md) — epoch declaration and migration rules
- [`signals/`](signals/) — signal inference, `silence`/`squelch`, capabilities
- [`analysis/portrait.md`](analysis/portrait.md) — the `compile/*` API in depth
- [`analysis/agent-reasoning.md`](analysis/agent-reasoning.md) — how agents query the model
- [`test-runner.md`](test-runner.md) — where `when!`/`unless!`/`gate!`/`%assert` are specified
- [`pipeline.md`](pipeline.md) — the full compile pipeline these operations hook into
