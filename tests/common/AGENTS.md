# tests/common

Shared test helpers for the Elle test suite.

## Responsibility

Provide canonical eval and setup functions so test files don't need to copy-paste their own variants. Includes:
- Fresh VM creation with primitives and stdlib
- Cached VM reuse for property tests (eliminates per-case bootstrap cost)
- Symbol table context management
- Proptest configuration respecting `PROPTEST_CASES` env var

Does NOT:
- Run tests (that's the test harness)
- Define test cases (that's individual test files)
- Manage test fixtures (that's `tests/fixtures/`)

## Key functions

### Fresh VM creation

**`eval_source(input: &str) -> Result<Value, String>`** — Evaluate Elle source through the full pipeline. Creates a fresh VM with primitives and stdlib on every call. Use this when you need a guaranteed-fresh VM (rare — prefer `eval_reuse` for property tests).

```rust
use crate::common::eval_source;
let result = eval_source("(+ 1 2)").unwrap();
assert_eq!(result, Value::int(3));
```

**`eval_source_bare(input: &str) -> Result<Value, String>`** — Same as `eval_source` but without stdlib. Creates a fresh VM on every call. Prelude macros (defn, let*, ->, ->>, when, unless, try/catch, etc.) are still available — they're loaded by `compile_all`'s internal `Expander::load_prelude`, not by `init_stdlib`. Use this for tests that never call stdlib functions (map, filter, fold, etc.).

```rust
use crate::common::eval_source_bare;
let result = eval_source_bare("(+ 1 2)").unwrap();
assert_eq!(result, Value::int(3));
```

**`setup() -> (SymbolTable, VM)`** — Returns an initialized (SymbolTable, VM) pair with primitives and stdlib registered. Sets the symbol table context but does NOT set VM context. Use this when you need direct access to the VM or symbol table (e.g., calling `analyze()` or `compile()` directly).

```rust
use crate::common::setup;
let (mut symbols, mut vm) = setup();
let result = analyze("(+ 1 2)", &mut symbols, &mut vm, "<test>").unwrap();
```

### Cached VM reuse (for property tests)

**`eval_reuse(input: &str) -> Result<Value, String>`** — Evaluate Elle source with a cached VM (primitives + stdlib). The VM is created once per thread and reused across calls. Between calls, the fiber is reset and globals are restored to their post-initialization snapshot. **Use this for property tests that need stdlib functions** (map, filter, reverse, etc.).

```rust
use crate::common::eval_reuse as eval_source;
let result = eval_source("(reverse (list 1 2 3))").unwrap();
```

**`eval_reuse_bare(input: &str) -> Result<Value, String>`** — Evaluate Elle source with a cached VM (primitives only, no stdlib). Same caching behavior as `eval_reuse`. **Use this for property tests that don't need stdlib** — this is the common case. Most property test files alias this as `eval_source`:

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

The cached VM approach eliminates per-case bootstrap cost:

1. **First call**: Create VM, register primitives, load stdlib, snapshot globals
2. **Subsequent calls**: Reset fiber, restore globals from snapshot, reuse VM
3. **Between cases**: Fiber is reset, globals are restored, JIT cache is cleared

This is safe because:
- Each test case is independent (globals are restored)
- Fiber state is reset (no cross-case contamination)
- JIT cache is cleared (no stale compiled code)

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~180 | `eval_source`, `eval_source_bare`, `eval_reuse`, `eval_reuse_bare`, `setup`, `proptest_cases` |

## Invariants

1. **Symbol table context must be set during stdlib init.** The `setup()` function sets context before `init_stdlib()` so that macros using `gensym` work correctly.

2. **Context must be cleared after use.** Leaving context pointers set can affect subsequent tests. `eval_source()` and `eval_source_bare()` clear context after use; `setup()` does not (caller is responsible).

3. **Cached VMs are thread-local.** Each thread has its own cache. No synchronization needed.

4. **Globals snapshot is taken post-initialization.** The snapshot includes all stdlib definitions. Between cases, globals are restored to this snapshot.

5. **Fiber is reset between cases.** `reset_fiber()` clears the operand stack and call stack, ensuring clean state for the next case.

6. **JIT cache is cleared between cases.** Prevents stale compiled code from affecting subsequent cases.

## When to use each function

| Function | When to use |
|----------|------------|
| `eval_source()` | Integration tests that need a guaranteed-fresh VM |
| `eval_source_bare()` | Integration tests that don't need stdlib |
| `eval_reuse()` | Property tests that need stdlib functions |
| `eval_reuse_bare()` | Property tests that don't need stdlib (common case) |
| `setup()` | Tests that need direct access to VM or SymbolTable |
| `proptest_cases()` | All property tests (inside `proptest!` block) |

## Common pitfalls

- **Using `eval_source()` in property tests**: Creates a fresh VM for every case, which is slow. Use `eval_reuse()` or `eval_reuse_bare()` instead.
- **Forgetting to set context in `setup()`**: If you call `setup()` and then use macros, set context manually: `set_vm_context(&mut vm as *mut VM); set_symbol_table(&mut symbols as *mut SymbolTable);`
- **Not clearing context after `setup()`**: If you use `setup()` and then call other test functions, clear context: `set_vm_context(std::ptr::null_mut()); set_symbol_table(std::ptr::null_mut());`
- **Assuming globals are isolated**: Globals are restored between cases, but if you modify a global and then call another test function, the modification persists. Use `eval_reuse()` or `eval_reuse_bare()` to ensure isolation.
