# tests/common

Shared test helpers for the Elle test suite.

## Responsibility

Provide canonical eval and setup functions so test files don't need to copy-paste their own variants. Includes:
- Fresh `Runtime` creation with primitives and stdlib
- Cached `RuntimeCore` reuse for property tests (eliminates per-case bootstrap cost)
- Proptest configuration respecting `PROPTEST_CASES` env var

Does NOT:
- Run tests (that's the test harness)
- Define test cases (that's individual test files)
- Manage test fixtures (that's `tests/fixtures/`)

Every helper drives a `Runtime` (`elle::runtime`), the one per-instance owner of the heap, `VM`, `SymbolTable`, and per-instance `CompileCtx`. The compile state each eval names is the instance's own (`rt.parts()`), so two test instances never share stdlib exports or REPL definitions. `Runtime` also points the VM at its own symbol table and `CompileCtx`, so executed code that resolves through the VM sees this instance's state.

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
| `mod.rs` | `eval_source`, `eval_source_bare`, `eval_source_unscheduled`, `eval_reuse`, `eval_reuse_bare`, `setup`, `proptest_cases` |

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
