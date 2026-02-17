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

## Function types

**NativeFn**: `fn(&[Value]) -> Result<Value, Condition>`
- Simple primitives that don't need VM access
- Return `Condition` for user-facing errors (type, arity, etc.)
- Examples: `+`, `car`, `string-length`

**VmAwareFn**: `fn(&[Value], &mut VM) -> LResult<Value>`
- Primitives that need to execute bytecode or access VM state
- Set `vm.current_exception` directly for user-facing errors, return `Ok(Value::NIL)`
- Return `Err(LError)` only for VM bugs
- Examples: `coroutine-resume`, `/`

## Adding a primitive

1. Create function in appropriate module
2. Register in that module's `register_*` function
3. That function is called by `registration.rs`

```rust
// In arithmetic.rs
pub fn prim_add(args: &[Value]) -> Result<Value, Condition> {
    // Implementation — return Err(Condition::type_error(...)) for errors
}

pub fn register_arithmetic(vm: &mut VM, symbols: &mut SymbolTable) {
    let sym = symbols.intern("+");
    vm.set_global(sym.0, Value::NativeFn(prim_add));
}
```

## Dependents

- `vm/mod.rs` - calls primitives during execution
- `repl.rs` - registers primitives at startup
- `main.rs` - registers primitives at startup

## Invariants

1. **Primitives validate arguments.** NativeFn returns `Condition::arity_error`
   or `Condition::type_error` on bad input. VmAwareFn sets `vm.current_exception`
   directly. Never panic.

2. **NativeFn returns `Result<Value, Condition>`.** VmAwareFn returns
   `LResult<Value>` but uses `vm.current_exception` for user-facing errors.
   Errors propagate; they're not swallowed.

3. **Symbol table pointers are set before use.** Some primitives (JIT, macros)
   need global access to symbol tables. Call `set_*_symbol_table` first.

4. **Thread-local state exists for some primitives.** JIT context, macro
   symbol table. Clean up with `clear_*` functions.

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
| `concurrency.rs` | `spawn`, `join`, `channel`, `send`, `receive` |
| `coroutines.rs` | `coroutine`, `coroutine-resume`, `coroutine-done?` |
| `exception.rs` | `throw`, `try`, exception utilities |
| `jit.rs` | `jit-compile`, `jit-compiled?` |
| `macros.rs` | `defmacro`, `macroexpand` |
| `introspection.rs` | `type-of`, `procedure?`, `arity` |
| `debug.rs` | `debug-print`, `trace` |

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | 38 | Re-exports |
| `registration.rs` | ~100 | `register_primitives` |
| `module_init.rs` | ~50 | `init_stdlib` |
| (others) | varies | Individual primitive implementations |
