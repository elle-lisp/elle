# Adding a New Primitive Function

<!-- audited: 2026-09-28 -->

A primitive is a Rust function callable from Elle, declared in its module's `primitive!` table.

### Files to modify

1. **`src/primitives/<module>.rs`** — Write the function and add its entry
   to the module's `primitive!` table.

2. **[registration.rs](../../src/primitives/registration.rs)** — Only for a
   new module file. Add the module's `PRIMITIVES` to `ALL_TABLES`.

3. **[mod.rs](../../src/primitives/mod.rs)** — Only for a new module file.
   Declare the module.

### Step 1: write the function

Every primitive has the type `PrimFn`:
`fn(&mut NativeCtx, &[Value]) -> (SignalBits, Value)`. The VM checks the
declared arity before the call, so the body can index the arguments it
declared.

```rust
use crate::primitives::ctx::NativeCtx;
use crate::value::fiber::{SignalBits, SIG_OK};
use crate::value::Value;

pub(crate) fn prim_my_even(ctx: &mut NativeCtx<'_>, args: &[Value]) -> (SignalBits, Value) {
    match args[0].as_int() {
        Some(n) => (SIG_OK, Value::bool(n % 2 == 0)),
        None => type_error!(ctx, args[0], "my/even?", "integer"),
    }
}
```

Build a heap result through the ctx (`ctx.string(..)`, `ctx.array(..)`,
`ctx.pair(..)`), so the value is born in the call's own region.

### Step 2: declare it

Add an entry to the module's `primitive!` table
([def.rs](../../src/primitives/def.rs) holds the macro and every field):

```rust
use crate::primitives::def::{RegionEffect, RetType};
use crate::signals::Signal;
use crate::value::types::Arity;

primitive! {
    // ... existing entries ...
    "my/even?" => prim_my_even {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "True if the integer is even.",
        params: &["n"],
        category: "my",
        example: "(my/even? 4) #=> true",
        effect: RegionEffect::Immediate,
        ret: RetType::Bool,
    }
}
```

Only the name and the function are required; every other field defaults to
`PrimitiveDef::DEFAULT`. Declare `effect` all the same:
`every_primitive_declares_an_examined_region_effect` fails on a primitive left
at the `Unknown` default. [effects.md](../impl/region/effects.md) says which
effect a body earns. Declare `ret` when the result type is fixed, because type
inference reads it.

### Step 3 (new module only)

Add the module's table to `ALL_TABLES` in
[registration.rs](../../src/primitives/registration.rs):

```rust
pub(crate) static ALL_TABLES: &[&[PrimitiveDef]] = &[
    // ... existing entries ...
    my_module::PRIMITIVES,
];
```

And declare the module in [mod.rs](../../src/primitives/mod.rs):

```rust
pub mod my_module;
```

### How it works

`register_primitives` in
[registration.rs](../../src/primitives/registration.rs) walks `ALL_TABLES`.
For each `PrimitiveDef` and each of its aliases, it interns the name into the
instance's symbol table and records the native-fn value, signal, arity, region
effect and return type in `PrimitiveMeta`. It also records the doc entry that
`(doc name)` prints. A native-fn value is an immediate whose payload is the
definition's `prim_id`.

### Key types

| Type | Location | Purpose |
|------|----------|---------|
| `PrimFn` | [types.rs](../../src/value/types.rs) | `fn(&mut NativeCtx, &[Value]) -> (SignalBits, Value)` |
| `NativeCtx` | [ctx.rs](../../src/primitives/ctx.rs) | The call's allocator, its VM, and its instance's memo |
| `PrimitiveDef` | [def.rs](../../src/primitives/def.rs) | Declarative metadata struct |
| `PrimitiveMeta` | [defmeta.rs](../../src/primitives/defmeta.rs) | Collected signals/arities maps |
| `Arity` | [types.rs](../../src/value/types.rs) | `Exact(n)`, `AtLeast(n)`, `Range(min, max)` |
| `Signal` | [signals](../../src/signals/mod.rs) | `Signal::silent()`, `Signal::errors()` |

### Conventions

- Return `(SIG_OK, value)` for success.
- Return `(SIG_ERROR, ctx.error("kind", "message"))` for an error, and
  `type_error!` for an argument of the wrong type.
- Every keyword the primitive returns needs a spelling, or it prints as
  `#<keyword:hash>` and `json/serialize` refuses the struct that carries it.
  A spelling fixed at build time — a struct key, an error kind, a status —
  goes in `VOCABULARY` in
  [vocabulary.rs](../../src/value/keyword/vocabulary.rs), and a test fails
  until it does. A spelling that exists only at run time is minted with
  `ctx.keyword(name)`, which records it in the instance memo. See
  [symbol.md](../impl/symbol.md).
- Read VM state through `ctx.vm()`. For what the dispatch loop owns, return
  `(SIG_RESUME, fiber_value)` or `(SIG_QUERY, ctx.pair(keyword, arg))`.

---

## See also

- [Cookbook index](index.md)
