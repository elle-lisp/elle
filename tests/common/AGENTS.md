# tests/common

<!-- audited: 2026-09-30 -->

Shared test helpers for the Elle test suite.

## Responsibility

Provide canonical eval and setup functions so test files don't need to copy-paste their own variants. Includes:
- Fresh `Runtime` creation with primitives and stdlib
- Cached `RuntimeCore` reuse for property tests (eliminates per-case bootstrap cost)
- Proptest configuration respecting `PROPTEST_CASES` env var
- A scratch directory under the platform temp root, removed on drop
- The documents `make doctest` runs, and the documents it must run
- Readers of the two Elle suites, of the Makefile, of the suite passes a target
  runs and of the workflow files, for the tests that check what CI runs, how it
  dimensions a run, and what it keeps
- A reader of an `elle test` session store, for the tests that check what a
  run records

Does NOT:
- Run tests (that's the test harness)
- Define test cases (that's individual test files)
- Manage test fixtures (that's `tests/fixtures/`)

Every eval drives a `Runtime` (`elle::runtime`), the one per-instance owner of the heap, `VM`, `SymbolTable`, and per-instance `CompileCtx`. The compile state each eval names is the instance's own (`rt.parts()`), so two test instances never share stdlib exports or REPL definitions. `Runtime` also points the VM at its own symbol table and `CompileCtx`, so executed code that resolves through the VM sees this instance's state.

## Key functions

### Fresh Runtime creation

**`eval_source<R>(input: &str, f: impl FnOnce(Result<Value, String>) -> R) -> R`** — Evaluate Elle source through the full pipeline and hand the result to `f` while the `Runtime` (and its heap) is still alive, tearing down only after `f` returns. A heap-valued result is a tagged pointer into that heap, so inspect it *inside* `f` and return only OWNED data (scalars, `String`, counts) — never the `Value` itself, which would dangle past teardown. Creates a fresh `Runtime` with primitives and stdlib on every call; use this when you need a guaranteed-fresh instance (rare — prefer `eval_reuse` for property tests).

```rust
use crate::common::eval_source;
eval_source("(+ 1 2)", |r| assert_eq!(r.unwrap(), Value::int(3)));
```

**`eval_source_bare<R>(input: &str, f: impl FnOnce(Result<Value, String>) -> R) -> R`** — Same as `eval_source` but without stdlib. Creates a fresh `Runtime` on every call. Prelude macros (defn, let*, ->, ->>, when, unless, try/catch, etc.) are still available — they're loaded into the `CompileCtx`'s expander, not by stdlib loading. Use this for tests that never call stdlib functions (map, filter, fold, etc.).

```rust
use crate::common::eval_source_bare;
eval_source_bare("(+ 1 2)", |r| assert_eq!(r.unwrap(), Value::int(3)));
```

**`eval_source_unscheduled<R>(input: &str, f: impl FnOnce(Result<Value, String>) -> R) -> R`** — Like `eval_source` (stdlib loaded) but runs without the async scheduler — a plain `vm.execute`, no `ev/run` wrapping. Same scoped-callback discipline (inspect inside `f`). Use this only for the rare test that asserts behavior outside a scheduler.

**`setup() -> Runtime`** — Returns an initialized `Runtime` with primitives and stdlib registered, contexts installed. Hand out the disjoint `(vm, symbols, cctx)` borrows via `rt.parts()`. Use this when you need direct access to the VM, symbol table, or compile context (e.g., calling `compile_file()` directly).

```rust
use crate::common::setup;
let mut rt = setup();
let (vm, symbols, cctx) = rt.parts();
```

### Cached reuse (for property tests)

**`eval_reuse(input: &str) -> Result<Value, String>`** — Evaluate Elle source with a cached `RuntimeCore` (primitives + stdlib). The core is created once per thread and reused across calls. Between calls the fiber is reset. **Use this for property tests that need stdlib functions** (map, filter, reverse, etc.).

```rust
use crate::common::eval_reuse as eval_source;
let result = eval_source("(reverse (list 1 2 3))").unwrap();
```

**`eval_reuse_bare(input: &str) -> Result<Value, String>`** — Evaluate Elle source with a cached `RuntimeCore` (primitives only, no stdlib). Same caching behavior as `eval_reuse`. **Use this for property tests that don't need stdlib** — this is the common case. Most property test files alias this as `eval_source`:

```rust
use crate::common::eval_reuse_bare as eval_source;
let result = eval_source("(+ 1 2)").unwrap();
assert_eq!(result, Value::int(3));
```

### Proptest configuration

**`proptest_cases(default: u32) -> ProptestConfig`** — Create a proptest config that respects the `PROPTEST_CASES` env var. When `PROPTEST_CASES` is set, its value overrides the given default. This lets CI and local development control case counts uniformly:

```bash
PROPTEST_CASES=8 cargo test    # fast smoke
cargo test                     # use per-test defaults
```

Usage in tests:

```rust
use crate::common::proptest_cases;

proptest! {
    #![proptest_config(proptest_cases(200))]

    #[test]
    fn my_invariant(n in -1000i64..1000) {
        let result = eval_source(&format!("(+ {} 1)", n)).unwrap();
        prop_assert_eq!(result, Value::int(n + 1));
    }
}
```

## Caching strategy

The cached `RuntimeCore` approach eliminates per-case bootstrap cost:

1. **First call**: Create the core, register primitives, load stdlib, build the `CompileCtx`
2. **Subsequent calls**: Reset the fiber, clear the JIT cache, reuse the core
3. **Between cases**: Fiber is reset and the JIT cache is cleared

`RuntimeCore` (not `Runtime`) is cached deliberately: a `Runtime` runs a teardown sweep on `Drop`, and a thread-local that drops at thread exit would run that sweep at an unpredictable point. `RuntimeCore` has no such `Drop`, so caching it is safe.

This is safe because:
- Fiber state is reset (no cross-case contamination)
- JIT cache is cleared (no stale compiled code)

## Files

| File | Content |
|------|---------|
| [mod.rs](mod.rs) | the evals (`eval_source`, `eval_source_bare`, `eval_source_unscheduled`, `eval_reuse`, `eval_reuse_bare`), `setup`, `proptest_cases`, `paint_stack`, and `ScratchDir`; it re-exports the readers of `repo.rs`, `passes.rs` and `workflows.rs` |
| [repo.rs](repo.rs) | the Makefile readers (`make_var`, `make_dry_run`, `make_dry_run_with`, `make_expand`, `makefile`) and the suite readers (`repo_root`, `suite`, `suite_files`, `declared_deadline`, `wide_patterns`) |
| [passes.rs](passes.rs) | the suite passes a target runs (`Pass`, `passes`), the two suites as sets (`lang_files`, `impl_files`), and `assert_plain_language_pass`, for `suites.rs` and `variants.rs` |
| [workflows.rs](workflows.rs) | the workflow readers (`workflow_files`, `workflow_jobs`, `runs_target`) |
| [documents.rs](documents.rs) | the documents `make doctest` runs (`doctest_documents`) and the documents it must run (`covered_documents`), for `doctest.rs` and `doctest_scope.rs` |
| [store.rs](store.rs) | the session-store readers (`query`, `scalar`), for the tests that drive `elle test` and read back what it recorded |

### Reading the Makefile

**`make_var(name, env)`** and **`make_dry_run(target)`** answer what `make`
itself will use: one variable's expanded value, and the commands a target will
run. **`make_dry_run_with(target, vars)`** sets variables on the command line
first, the way a CI job does. A test about what a CI pass runs reads through
these rather than parsing the Makefile, because a parser that resolves
variables, `ifdef`s and `$(shell …)` is a second `make` that disagrees with the
first.
**`make_expand(name)`** is `make_var` with the failure spelled out, for a test
that cannot proceed without the value.

### Reading the suites

**`suite(dir)`** answers the files a suite runs, the `.lisp` files at the top of
`tests/lang` or `tests/impl`, and **`suite_files()`** answers both.
**`declared_deadline(path)`** reads the deadline a file gives itself, and
**`wide_patterns()`** the families the Makefile gives a wider budget. Several
test files ask those questions, and a second copy of a reader is a second
answer.

**`paint_stack(pattern, depth)`** fills stack frames with a byte pattern, so a
determinism test can prove an artifact carries none of what a construction
temporary held.

**`ScratchDir::new(tag)`** is a uniquely-named directory under the platform temp
root, removed on drop — the panic path included. Never write a test file under a
hardcoded `/tmp`; `tests/integration/scratch.rs` fails the build over it.

### Reading the workflows

**`workflow_files()`** answers every file under `.github/workflows`, and
**`workflow_jobs(text)`** splits one into (name, body) pairs with the comment
lines dropped — a job that discusses a command it does not run must not read as
a job that runs it. **`runs_target(body, target)`** asks whether a job runs one
make target, and not a longer one it prefixes. `workflows.rs` asks what the gate
waits for and `run_artifacts.rs` asks what a suite job leaves behind, so the
reading lives here rather than twice.

### Reading a session store

**`query(db, sql)`** answers the rows `elle test --query` prints, and
**`scalar(db, sql)`** the one integer a query aliases `c`.

## Invariants

1. **Each cached core points its VM at its own symbol table.** `RuntimeCore::bare` wires this up before stdlib load, so stdlib-load gensym (and all name resolution) resolves through it.

2. **Cached cores are thread-local.** Each thread has its own cache. No synchronization needed.

3. **Fiber is reset between cases.** `reset_fiber()` clears the operand stack and call stack, ensuring clean state for the next case.

4. **JIT cache is cleared between cases.** Prevents stale compiled code from affecting subsequent cases.

## When to use each function

| Function | When to use |
|----------|------------|
| `eval_source()` | Integration tests that need a guaranteed-fresh Runtime |
| `eval_source_bare()` | Integration tests that don't need stdlib |
| `eval_source_unscheduled()` | Tests asserting behavior outside the async scheduler |
| `eval_reuse()` | Property tests that need stdlib functions |
| `eval_reuse_bare()` | Property tests that don't need stdlib (common case) |
| `setup()` | Tests that need direct access to VM, SymbolTable, or CompileCtx |
| `proptest_cases()` | All property tests (inside `proptest!` block) |

## Common pitfalls

- **Using `eval_source()` in property tests**: Creates a fresh Runtime for every case, which is slow. Use `eval_reuse()` or `eval_reuse_bare()` instead.
- **Holding `rt.parts()` borrows too long**: `parts()` hands out disjoint `&mut` borrows of the VM, symbol table, and compile context; finish using them before calling `parts()` again.
