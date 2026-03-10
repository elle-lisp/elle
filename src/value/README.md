# Value

Runtime value representation using NaN-boxing.

## Overview

The `value` module defines the `Value` type — an 8-byte NaN-boxed representation that encodes all Elle runtime values: integers, floats, booleans, strings, lists, arrays, tables, closures, fibers, sets, and more.

## Value Types

| Type | Display | Mutable | Purpose |
|------|---------|---------|---------|
| Integer | `42` | — | 48-bit signed integer |
| Float | `3.14` | — | IEEE 754 double |
| Boolean | `true`, `false` | — | Truth value |
| Nil | `nil` | — | Absence (falsy) |
| Empty list | `()` | — | Empty list (truthy) |
| Keyword | `:key` | — | Interned keyword |
| Symbol | `name` | — | Interned symbol |
| String | `"hello"` | — | Immutable UTF-8 string |
| Cons | `(1 . 2)` | — | List pair |
| Array | `[1 2 3]` | — | Immutable fixed-length sequence |
| @Array | `@[1 2 3]` | ✓ | Mutable variable-length sequence |
| Struct | `{:key val}` | — | Immutable key-value map |
| @Struct | `@{:key val}` | ✓ | Mutable key-value map |
| Set | `\|1 2 3\|` | — | Immutable set |
| @Set | `@\|1 2 3\|` | ✓ | Mutable set |
| @String | `@"bytes"` | ✓ | Mutable byte sequence |
| Bytes | `#bytes[...]` | — | Immutable binary data |
| @Bytes | `#@bytes[...]` | ✓ | Mutable binary data |
| Closure | `<closure>` | — | Function with captured environment |
| Fiber | `<fiber:alive>` | — | Independent execution context |
| Cell | `<cell 42>` | ✓ | Mutable box for captured variables |
| Parameter | `<parameter:0>` | — | Dynamic binding |

## Creating Values

Use constructor methods, not enum variants:

```rust
Value::int(42)
Value::float(3.14)
Value::bool(true)
Value::string("hello")
Value::cons(car, cdr)
Value::array(vec![...])
Value::table(btree_map)
Value::set(btree_set)
Value::set_mut(btree_set)
Value::closure(closure)
Value::fiber(handle)
```

## Accessing Values

Use accessor methods to extract values:

```rust
value.as_int()           // Option<i64>
value.as_float()         // Option<f64>
value.as_bool()          // Option<bool>
value.as_string()        // Option<&str>
value.as_cons()          // Option<&Cons>
value.as_array()         // Option<&RefCell<Vec<Value>>>
value.as_table()         // Option<&RefCell<BTreeMap<...>>>
value.as_set()           // Option<&BTreeSet<Value>>
value.as_set_mut()       // Option<&RefCell<BTreeSet<Value>>>
value.as_closure()       // Option<&Rc<Closure>>
value.as_fiber()         // Option<&FiberHandle>
```

Type predicates:

```rust
value.is_int()
value.is_float()
value.is_bool()
value.is_string()
value.is_cons()
value.is_array()
value.is_table()
value.is_set()
value.is_set_mut()
value.is_closure()
value.is_fiber()
```

## Sets

Two set types exist, following the immutable/mutable split:

- **Immutable set** (`LSet`): `BTreeSet<Value>`, no `RefCell`. Display: `|1 2 3|`. Type name: `"set"`.
- **Mutable set** (`LSetMut`): `RefCell<BTreeSet<Value>>`. Display: `@|1 2 3|`. Type name: `"@set"` (or `:@set` as keyword).

Set membership uses structural equality. When a mutable value is inserted into a set, it is frozen (converted to its immutable equivalent) to prevent mutation from breaking set invariants.

## Heap Objects

All non-immediate values are heap-allocated via `Rc`. Mutable heap objects use `RefCell` for interior mutability:

- `Cons` — list pair
- `LArrayMut` — mutable vector
- `LStructMut` — mutable BTreeMap
- `LStruct` — immutable BTreeMap
- `LArray` — immutable vector
- `LStringMut` — mutable byte vector
- `LBytes` — immutable byte vector
- `LBytesMut` — mutable byte vector
- `Closure` — bytecode + environment
- `Fiber` — execution context
- `Cell` — mutable box
- `LSet` — immutable set
- `LSetMut` — mutable set
- `Parameter` — dynamic binding
- `Syntax` — compile-time syntax object
- `Binding` — compile-time binding metadata

## Invariants

1. **`Value` is `Copy`.** All 8 bytes fit in a register. Heap data is `Rc`.

2. **`nil` ≠ empty list.** `Value::NIL` is falsy (absence). `Value::EMPTY_LIST` is truthy (empty list). Lists terminate with `EMPTY_LIST`, not `NIL`.

3. **Two cell types exist.** `Cell` (user-created via `box`, explicit deref) and `LocalCell` (compiler-created for mutable captures, auto-unwrapped).

4. **Mutable values freeze on set insertion.** When a mutable value is inserted into a set, it is converted to its immutable equivalent.

## See Also

- [AGENTS.md](AGENTS.md) — technical reference for LLM agents
- [`repr/`](repr/) — NaN-boxing implementation
- [`heap.rs`](heap.rs) — heap-allocated object types
- [`closure.rs`](closure.rs) — closure representation
- [`fiber.rs`](fiber.rs) — fiber (coroutine) implementation
