# CPS Interpreter Design: Stateless with Index-Based Locals

## Problem

Issues #251-254 stem from the CPS interpreter losing local variable bindings across yield/resume:

- **#251**: `let` bindings lost on resume
- **#252**: Call stack not preserved for nested yielding calls
- **#253**: Lambda creation not implemented in CPS
- **#254**: Recursive yielding functions fail (same root cause as #252)

The root cause: `CpsInterpreter` stores locals in a `HashMap<SymbolId, Value>` that isn't preserved in continuations.

## Solution

Convert to a stateless interpreter with index-based locals:

1. **Compute indices at transform time** - CPS transformer assigns indices to `let` and `for` bindings
2. **Pre-size environment** - Allocate all slots at closure/coroutine entry
3. **Use `Rc<RefCell<Vec<Value>>>`** - Shared mutable environment, no cloning on yield
4. **Stateless eval function** - All state in env and continuation, nothing in interpreter

## Environment Layout

```
[captures..., params..., locals...]
 ^            ^          ^
 0            num_caps   num_caps+num_params
```

- `num_locals` = total size = num_captures + num_params + num_local_vars
- Computed by CPS transformer, stored in `CpsExpr::Lambda`

## Changes

### CpsExpr (cps_expr.rs)

```rust
// Let: SymbolId -> index
Let {
    index: usize,
    init: Box<CpsExpr>,
    body: Box<CpsExpr>,
}

// For: SymbolId -> index
For {
    index: usize,
    iter: Box<CpsExpr>,
    body: Box<CpsExpr>,
    continuation: Rc<Continuation>,
}

// Lambda: add num_locals
Lambda {
    params: Vec<SymbolId>,
    body: Box<CpsExpr>,
    captures: Vec<(SymbolId, usize, usize)>,
    num_locals: usize,
}
```

### CPS Transformer (transform.rs)

Add scope tracking:

```rust
pub struct CpsTransformer<'a> {
    effect_ctx: &'a EffectContext,
    next_local_index: usize,
}
```

- `transform_let`: assigns index, increments `next_local_index`
- `transform_for`: assigns index for loop variable
- `transform_lambda`: saves/restores `next_local_index`, computes `num_locals`
- `transform_closure_body`: entry point, sets initial index = num_captures + num_params

### Continuation (continuation.rs)

Add environment-carrying variant:

```rust
WithEnv {
    env: Rc<RefCell<Vec<Value>>>,
    inner: Rc<Continuation>,
}
```

### Interpreter (interpreter.rs)

Rewrite as stateless functions:

```rust
pub fn eval(
    vm: &mut VM,
    expr: &CpsExpr,
    env: &Rc<RefCell<Vec<Value>>>,
) -> Result<Action, String>
```

Key cases:

- `Let`: `env.borrow_mut()[index] = val`
- `Var`: `env.borrow()[index].clone()`
- `Yield`: wrap continuation with `WithEnv { env: env.clone(), inner }`
- `CpsCall`: snapshot env in `CallReturn`, restore on return
- `Lambda`: create `Value::Closure` with `cps_body` field

### Closure (value.rs)

Add CPS body storage:

```rust
pub struct Closure {
    // ... existing fields ...
    pub cps_body: Option<Rc<CpsExpr>>,  // NEW: for CPS-created closures
}
```

When calling a closure:
- If `cps_body.is_some()` and in CPS context → use CPS interpreter
- Otherwise → use bytecode VM

### Trampoline (trampoline.rs)

- Update `apply_continuation_with_vm` to take `&Rc<RefCell<Vec<Value>>>`
- Handle `WithEnv`: continue with restored environment
- Handle `CallReturn`: wrap restored env in RefCell

### Primitives (primitives.rs)

Update `make_coroutine`:

```rust
let mut env = Vec::with_capacity(closure.num_locals);
env.extend(closure.env.iter().cloned());
for _ in closure.num_captures..closure.num_locals {
    env.push(Value::Nil);
}
let env = Rc::new(RefCell::new(env));
```

## Files Modified

| File | Changes |
|------|---------|
| `src/compiler/cps/cps_expr.rs` | index in Let/For, num_locals in Lambda |
| `src/compiler/cps/transform.rs` | Scope tracking, index computation |
| `src/compiler/cps/continuation.rs` | WithEnv variant |
| `src/compiler/cps/interpreter.rs` | Stateless functions, RefCell env |
| `src/compiler/cps/trampoline.rs` | New env type, WithEnv handling |
| `src/compiler/cps/primitives.rs` | Pre-size env |
| `src/value.rs` | cps_body field in Closure |

## Testing

1. Enable disabled tests for #251-254
2. Add unit tests for:
   - Let binding preserved across yield
   - Nested let bindings
   - For loop variable across yield
   - Lambda creation in coroutine
   - Nested CPS calls with yield
   - Recursive yielding functions

## Why This Design

- **Stateless**: Nothing to forget across yield/resume
- **Index-based**: O(1) access, matches bytecode VM approach
- **Pre-sized**: No allocations during eval
- **RefCell**: Cheaper than clone-on-write, explicit mutability
- **Same Closure type**: CPS closures are regular closures with `cps_body` set
