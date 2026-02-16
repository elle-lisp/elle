# Lexical Binding Resolution

This module resolves variable references at compile time. Given a symbol like
`x`, it determines: is this a local variable? A captured upvalue? A global?

The answer is encoded as a `VarRef`, which the compiler uses to emit the
appropriate bytecode instruction.

## Why This Exists

Lexical scoping means variable resolution is static - we can determine at
compile time exactly where each variable lives. This is faster than runtime
lookup and enables closure capture analysis.

Consider:

```lisp
(let ((x 10))
  (fn () x))  ; x is captured - needs upvalue
```

The inner `x` isn't a local (it's outside the lambda) and isn't a global
(it's bound by `let`). This module tracks scope nesting to classify it
correctly as an upvalue.

## Key Concepts

**VarRef variants:**
- `Local` - in current function's frame (parameters, local `let` bindings)
- `LetBound` - outside any function, uses runtime scope stack
- `Upvalue` - captured from enclosing function
- `Global` - top-level binding

**Capture tracking:**
When a nested lambda references an outer variable, we mark it `captured`.
When `set!` mutates a variable, we mark it `mutated`. Variables that are
both captured AND mutated need special handling (cell boxing) to maintain
correct semantics across closure boundaries.

## Usage

```rust
let mut scopes = ScopeStack::new();

// Enter a function
scopes.push(true, 0);  // is_function=true, base_index=0
let x_idx = scopes.bind(x_symbol);  // returns slot index

// Later, when we see a reference to x
match scopes.lookup(x_symbol) {
    Some((scope_idx, binding)) => {
        // Found it - determine if local, upvalue, etc.
    }
    None => {
        // Must be global (or undefined)
    }
}
```

## See Also

- [AGENTS.md](AGENTS.md) - technical reference for LLM agents
- `src/hir/binding.rs` - HIR-level binding representation
- `src/vm/scope/` - runtime scope management
