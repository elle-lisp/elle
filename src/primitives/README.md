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

## Unified Function Type

All primitives use a single type: `fn(&[Value]) -> (SignalBits, Value)`.

```rust
fn prim_add(args: &[Value]) -> (SignalBits, Value) {
    let mut sum = 0i64;
    for arg in args {
        match arg.as_int() {
            Some(n) => sum += n,
            None => return (SIG_ERROR, Value::condition(
                Condition::type_error("expected integer".to_string())
            )),
        }
    }
    (SIG_OK, Value::int(sum))
}
```

No primitive has VM access. Operations that need the VM
(fiber execution) return `(SIG_RESUME, fiber_value)` and the
VM's dispatch loop handles the actual execution.

## Adding a New Primitive

1. **Choose the right module** based on category (arithmetic, string, etc.)

2. **Implement the function**:
   ```rust
   pub fn prim_my_func(args: &[Value]) -> (SignalBits, Value) {
       if args.len() != 2 {
           return (SIG_ERROR, Value::condition(
               Condition::arity_error("my-func: expected 2 arguments".to_string())
           ));
       }
       // Implementation...
       (SIG_OK, result)
   }
   ```

3. **Register it** in `registration.rs`:
   ```rust
   register_fn(vm, symbols, &mut effects, "my-func", prim_my_func, Effect::raises());
   ```

## Error Handling

All errors use `(SIG_ERROR, Value::condition(...))`:

```rust
// Type mismatch
return (SIG_ERROR, Value::condition(Condition::type_error("expected integer".to_string())));

// Wrong number of arguments
return (SIG_ERROR, Value::condition(Condition::arity_error("expected 2 arguments".to_string())));
```

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- `src/vm/call.rs` - dispatches primitive calls, handles signal bits
- `docs/builtins.md` - user-facing primitive documentation
