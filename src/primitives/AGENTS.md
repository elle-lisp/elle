# primitives

Built-in functions. Registered into the VM at startup.

## Responsibility

Implement Elle's standard library of built-in functions:
- Arithmetic, comparison, logic
- List and vector operations
- String manipulation
- I/O and file operations
- Concurrency primitives
- FFI support functions
- Introspection and debugging

Does NOT:
- Define special forms (those are in `hir/analyze.rs`)
- Execute bytecode (that's `vm`)
- Compile code (that's `compiler`, `hir`, `lir`)

## Interface

| Function | Purpose |
|----------|---------|
| `register_primitives(vm, symbols)` | Install all primitives |
| `init_stdlib(vm, symbols)` | Load stdlib.lisp |

## Function type

**NativeFn**: `fn(&[Value]) -> (SignalBits, Value)`

All primitives use a single unified type. No primitive has VM access.
Return values:
- `(SIG_OK, value)` — success, push value onto stack
- `(SIG_ERROR, error_val(kind, msg))` — error, stored in `fiber.signal`
- `(SIG_RESUME, fiber_value)` — fiber resume, VM handles fiber swap
- `(SIG_QUERY, cons(keyword, arg))` — VM state query, dispatched by `dispatch_query()` in `signal.rs`

All SIG_RESUME primitives (including coroutine wrappers) return
`(SIG_RESUME, fiber_value)`. Fiber primitives (`fiber/resume`) return SIG_RESUME with the fiber value.
The VM swaps the child fiber into `vm.fiber`, executes it, then swaps back.
`fiber/signal` returns the signal bits directly — the VM's catch-all handler
stores them in `fiber.signal` and suspends the fiber.

## Adding a primitive

1. Create function in appropriate module
2. Register in that module's `register_*` function
3. That function is called by `registration.rs`

```rust
// In arithmetic.rs
pub fn prim_add(args: &[Value]) -> (SignalBits, Value) {
    // Implementation — return (SIG_ERROR, error_val("type-error", "msg")) for errors
}

pub fn register_arithmetic(vm: &mut VM, symbols: &mut SymbolTable) {
    let sym = symbols.intern("+");
    vm.set_global(sym.0, Value::native_fn(prim_add));
}
```

## Dependents

- `vm/call.rs` - dispatches primitive calls, handles signal bits
- `repl.rs` - registers primitives at startup
- `main.rs` - registers primitives at startup

## Invariants

1. **Primitives validate arguments.** Return `(SIG_ERROR, error_val(kind, msg))`
   for arity or type errors. Never panic.

2. **All primitives return `(SignalBits, Value)`.** No exceptions. Errors are
   signaled via SIG_ERROR with an error tuple `[:keyword "message"]`.

3. **No primitive has VM access.** Operations that need the VM (fiber
   execution) return SIG_RESUME and let the VM dispatch loop handle it.

4. **Symbol table pointers are set before use.** Some primitives (list length)
   need global access to symbol tables. Call `set_*_symbol_table` first.

5. **Thread-local state exists for some primitives.** Length symbol table.
   Clean up with `clear_*` functions.

## Modules

| Module | Contains |
|--------|----------|
| `arithmetic.rs` | `+`, `-`, `*`, `/`, `mod` |
| `comparison.rs` | `=`, `<`, `>`, `<=`, `>=` |
| `logic.rs` | `not`, `and`, `or` (functions, not special forms) |
| `list.rs` | `cons`, `car`, `cdr`, `list`, `length`, `append` |
| `vector.rs` | `vector`, `vector-ref`, `vector-set!`, `vector-length` |
| `string.rs` | `string-length`, `string-append`, `substring` |
| `table.rs` | `table`, `table-get`, `table-set!` |
| `structs.rs` | `struct`, `struct-get` |
| `file_io.rs` | `read-file`, `write-file`, `file-exists?` |
| `display.rs` | `print`, `println`, `display` |
| `type_check.rs` | `nil?`, `pair?`, `number?`, `string?`, etc. |
| `higher_order.rs` | `map`, `filter`, `fold`, `apply` |
| `concurrency.rs` | `spawn`, `join`, `current-thread-id` |
| `coroutines.rs` | `coro/new`, `coro/resume`, `coro/done?`, `coro/status`, `coro/value`, `coro?>iterator`, `yield-from` |
| `fibers.rs` | `fiber/new`, `fiber/resume`, `fiber/signal`, `fiber/status`, `fiber/value`, `fiber/bits`, `fiber/mask`, `fiber/parent`, `fiber/child`, `fiber/propagate`, `fiber/cancel`, `fiber?` |
| `time.rs` | `clock/monotonic`, `clock/realtime`, `clock/cpu`, `time/sleep` |
| `time_def.rs` | `time/stopwatch`, `time/elapsed` (Elle definitions via `eval`) |
| `meta.rs` | `gensym` (runtime unique symbol generation) |
| `debugging.rs` | `closure?`, `jit?`, `pure?`, `coro?`, `mutates-params?`, `raises?`, `arity`, `captures`, `bytecode-size`, `call-count`, `global?`, `string->keyword`, `disbit`, `disjit` |
| `debug.rs` | `debug-print`, `trace`, `memory-usage` |

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 35 | Re-exports |
| `registration.rs` | ~1390 | `register_primitives`, `register_fn` |
| `module_init.rs` | ~170 | `init_stdlib`, module initialization |
| (others) | varies | Individual primitive implementations |
