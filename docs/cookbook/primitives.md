# Adding a New Primitive Function


A primitive is a Rust function callable from Elle. All primitives have the
same signature: `fn(&[Value]) -> (SignalBits, Value)`.

### Files to modify

1. **`src/primitives/<module>.rs`** — Implement the function and add it to
   the module's `PRIMITIVES` table.

2. **`src/primitives/registration.rs`** — Only if creating a *new module
   file*. Add the module's `PRIMITIVES` to the `ALL_TABLES` array.

3. **`src/primitives/mod.rs`** — Only if creating a *new module file*. Add
   `pub mod <module>;`.

### Step by step

**Step 1: Write the function** in the appropriate module (e.g.,
`src/primitives/string.rs` for string operations).

```rust
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};

pub fn prim_my_func(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val("arity-error",
                format!("my-func: expected 1 argument, got {}", args.len())),
        );
    }
    // ... implementation ...
    (SIG_OK, Value::int(42))
}
```

**Step 2: Add to the module's `PRIMITIVES` table** (a `const &[PrimitiveDef]`
at the bottom of the file):

```rust
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::types::Arity;

pub const PRIMITIVES: &[PrimitiveDef] = &[
    // ... existing entries ...
    PrimitiveDef {
        name: "my-func",
        func: prim_my_func,
        signal: Signal::silent(),       // or Signal::yields() if it signals
        arity: Arity::Exact(1),
        doc: "One-line description.",
        params: &["x"],
        category: "my-category",
        example: "(my-func 42) #=> 42",
        aliases: &[],
        ..PrimitiveDef::DEFAULT
    },
];
```

**Step 3 (new module only):** Add the module to `ALL_TABLES` in
`src/primitives/registration.rs`:

```rust
pub(crate) const ALL_TABLES: &[&[PrimitiveDef]] = &[
    // ... existing entries ...
    my_module::PRIMITIVES,
];
```

And declare it in `src/primitives/mod.rs`:

```rust
pub mod my_module;
```

### How it works

`register_primitives()` in `registration.rs` iterates `ALL_TABLES`. For
each `PrimitiveDef`, it:
- Interns the name via `symbols.intern(def.name)` → `SymbolId`
- Stores `Value::native_fn(def.func)` in `vm.globals[sym_id]`
- Records signal and arity in `PrimitiveMeta`
- Registers aliases identically

At runtime, the VM fetches the `NativeFn` value from globals and `Call`
dispatches it via `handle_primitive_signal()` in `src/vm/signal.rs`.

### Key types

| Type | Location | Purpose |
|------|----------|---------|
| `NativeFn` | `src/value/types.rs` | `fn(&[Value]) -> (SignalBits, Value)` |
| `PrimitiveDef` | `src/primitives/def.rs` | Declarative metadata struct |
| `PrimitiveMeta` | `src/primitives/def.rs` | Collected signals/arities maps |
| `Arity` | `src/value/types.rs` | `Exact(n)`, `AtLeast(n)`, `Range(min, max)` |
| `Signal` | `src/signals/` | `Signal::silent()`, `Signal::yields()` |

### Conventions

- Return `(SIG_OK, value)` for success.
- Return `(SIG_ERROR, error_val("kind", "message"))` for errors.
- Validate arity and types at the top of the function.
- No primitive has VM access. If you need VM interaction, return
  `(SIG_RESUME, fiber_value)` or `(SIG_QUERY, cons(keyword, arg))`.

---

---

## See also

- [Cookbook index](index.md)
