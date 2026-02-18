# Primitives

Primitives are Elle's built-in functions - operations that can't be
implemented in Elle itself because they need access to runtime internals
or external resources.

## Using Primitives

Primitives are automatically available after VM initialization:

```lisp
(+ 1 2 3)           ; Arithmetic
(car '(a b c))      ; List operations
(string-length "hi") ; String operations
(print "Hello!")    ; I/O
```

## Two Function Types

**NativeFn**: Simple functions that operate on values.

```rust
fn prim_add(args: &[Value]) -> Result<Value, Condition> {
    let mut sum = 0i64;
    for arg in args {
        sum += arg.as_int()?;
    }
    Ok(Value::int(sum))
}
```

**VmAwareFn**: Functions that need VM access (for executing closures,
managing coroutines, etc.).

```rust
fn prim_apply(args: &[Value], vm: &mut VM) -> LResult<Value> {
    let func = &args[0];
    let arg_list = args[1].list_to_vec()?;
    // Need VM to call the function
    vm.call_function(func, &arg_list)
}
```

## Adding a New Primitive

1. **Choose the right module** based on category (arithmetic, string, etc.)

2. **Implement the function**:
   ```rust
   pub fn prim_my_func(args: &[Value]) -> Result<Value, Condition> {
       if args.len() != 2 {
           return Err(Condition::arity_error(2, args.len()));
       }
       // Implementation...
   }
   ```

3. **Register it**:
   ```rust
    pub fn register_my_module(vm: &mut VM, symbols: &mut SymbolTable) {
        let sym = symbols.intern("my-func");
        vm.set_global(sym.0, Value::native_fn(prim_my_func));
    }
   ```

4. **Call from `registration.rs`**:
   ```rust
   pub fn register_primitives(vm: &mut VM, symbols: &mut SymbolTable) {
       // ... existing registrations ...
       my_module::register_my_module(vm, symbols);
   }
   ```

## Variadic Functions

Use `Arity::AtLeast(n)` for functions that take a minimum number of arguments:

```rust
// (+ a b ...) - at least 0 arguments
fn prim_add(args: &[Value]) -> Result<Value, Condition> {
    args.iter().try_fold(0i64, |acc, v| {
        Ok(acc + v.as_int()?)
    }).map(Value::int)
}
```

## Error Handling

NativeFn uses `Result<Value, Condition>` with `Condition` builders:

```rust
// Type mismatch
return Err(Condition::type_error("integer", value.type_name()));

// Wrong number of arguments
return Err(Condition::arity_error(2, args.len()));
```

VmAwareFn uses `LResult<Value>` but sets `vm.current_exception` for user-facing
errors and returns `Ok(Value::NIL)`. Only return `Err(LError)` for VM bugs.

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- `src/vm/` - executes primitive calls
- `docs/BUILTINS.md` - user-facing primitive documentation
