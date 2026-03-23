# value/repr

Tagged-union representation: Value constructors, accessors, and trait implementations.

## Responsibility

- Define the `Value` type (16-byte tagged-union representation)
- Provide constructors for immediate and heap-allocated values
- Provide accessors and type checking predicates
- Implement `Display`, `Debug`, `Clone`, `PartialEq` for values
- Handle tag/payload encoding/decoding

Does NOT:
- Allocate heap objects (that's `value/heap`)
- Execute code (that's `vm`)
- Manage fibers (that's `value/fiber`)

## Key types

| Type | Purpose |
|------|---------|
| `Value` | 16-byte tagged-union value `(tag: u64, payload: u64)` (Copy) |

## Tagged-union encoding

`Value` is a `(tag: u64, payload: u64)` pair (16 bytes total):

| Tag | Payload | Type |
|-----|---------|------|
| TAG_INT | i64 | Integer (full-range i64) |
| TAG_FLOAT | f64 bits | Float |
| TAG_NIL | 0 | Nil (falsy) |
| TAG_FALSE | 0 | False (falsy) |
| TAG_TRUE | 0 | True (truthy) |
| TAG_EMPTY_LIST | 0 | Empty list (truthy) |
| TAG_SYMBOL | u32 symbol ID | Symbol |
| TAG_KEYWORD | FNV-1a hash of name | Keyword |
| TAG_PTR | heap pointer | Cons, Array, Table, Closure, Fiber, etc. |

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~280 | Tagged-union Value type, tag encoding, constants |
| `constructors.rs` | ~380 | Value construction methods (int, float, bool, symbol, keyword, cons, array, table, closure, fiber, etc.) |
| `accessors.rs` | ~670 | Value field access and type checking (is_int, as_int, is_string, as_string, etc.) |
| `traits.rs` | ~150 | Display, Debug, Clone implementations |
| `tests.rs` | ~100 | Value encoding roundtrip tests |

## Constructors

| Constructor | Type | Notes |
|-------------|------|-------|
| `Value::int(n)` | Int | Full-range i64 |
| `Value::float(f)` | Float | Handles NaN/Infinity specially |
| `Value::bool(b)` | Bool | True or False |
| `Value::symbol(id)` | Symbol | From SymbolId |
| `Value::keyword(name)` | Keyword | FNV-1a hash of name; registers in global name table |
| `Value::nil()` | Nil | Falsy, represents absence |
| `Value::EMPTY_LIST` | EmptyList | Truthy, represents empty list |
| `Value::TRUE` | True | Truthy singleton |
| `Value::FALSE` | False | Falsy singleton |
| `Value::UNDEFINED` | Undefined | Truthy singleton |
| `Value::cons(car, cdr)` | Cons | Heap-allocated cons cell |
| `Value::array(elems)` | @array | Heap-allocated mutable @array |
| `Value::@struct(entries)` | @struct | Heap-allocated mutable @struct |
| `Value::array(elems)` | array | Heap-allocated immutable array |
| `Value::struct_(entries)` | struct | Heap-allocated immutable struct |
| `Value::string(s)` | string | Heap-allocated immutable string |
| `Value::@string(bytes)` | @string | Heap-allocated mutable @string |
| `Value::bytes(bytes)` | bytes | Heap-allocated immutable byte sequence |
| `Value::@bytes(bytes)` | @bytes | Heap-allocated mutable @bytes |
| `Value::closure(c)` | Closure | Bytecode + env + arity + signal + location_map |
| `Value::fiber(f)` | Fiber | Independent execution context |
| `Value::lbox(v)` | LBox | Mutable lbox (user-created via `box`) |
| `Value::binding(name, scope)` | Binding | Compile-time binding metadata (never at runtime) |
| `Value::syntax(s)` | Syntax | Syntax object with scope sets (for macro expansion) |
| `Value::parameter(id, default)` | Parameter | Dynamic binding |
| `Value::external(obj)` | External | Opaque plugin-provided Rust object |

## Accessors

| Accessor | Returns | Notes |
|----------|---------|-------|
| `is_int()` | bool | Type check |
| `as_int()` | Option<i64> | Extract integer |
| `is_float()` | bool | Type check |
| `as_float()` | Option<f64> | Extract float |
| `is_bool()` | bool | Type check |
| `as_bool()` | Option<bool> | Extract boolean |
| `is_symbol()` | bool | Type check |
| `as_symbol()` | Option<SymbolId> | Extract symbol ID |
| `is_keyword()` | bool | Type check |
| `keyword_hash()` | Option<u64> | Extract 47-bit keyword hash (fast path — no lock) |
| `as_keyword_name()` | Option<String> | Extract keyword name (acquires RwLock + allocates) |
| `is_nil()` | bool | Type check (only matches nil, not empty list) |
| `is_empty_list()` | bool | Type check (only matches empty list, not nil) |
| `is_cons()` | bool | Type check |
| `as_cons()` | Option<(Value, Value)> | Extract car and cdr |
| `is_array()` | bool | Type check |
| `as_array()` | Option<Rc<RefCell<Vec<Value>>>> | Extract array |
| `is_table()` | bool | Type check |
| `as_table()` | Option<Rc<RefCell<HashMap<...>>>> | Extract table |
| `is_string()` | bool | Type check |
| `as_string()` | Option<&str> | Extract string |
| `is_closure()` | bool | Type check |
| `as_closure()` | Option<Rc<Closure>> | Extract closure |
| `is_fiber()` | bool | Type check |
| `as_fiber()` | Option<FiberHandle> | Extract fiber |
| `type_name()` | &'static str | Human-readable type name |
| `is_truthy()` | bool | Truthiness check (nil and false are falsy) |

## Invariants

1. **`Value` is `Copy`.** All 16 bytes (tag + payload). Heap data is `Rc`.

2. **Trait tables are invisible to equality and ordering.** The `traits` field
   on heap variants is **not compared** by `PartialEq`, not hashed by `Hash`,
   and not compared by `Ord`. Trait identity is a separate concern checked via
   `identical?`.

3. **`nil` ≠ empty list.** `Value::NIL` is falsy (absence). `Value::EMPTY_LIST` is truthy (empty list). Lists terminate with `EMPTY_LIST`, not `NIL`.

4. **Two lbox types exist.** `LBox` (user-created via `box`, explicit deref) and `LocalLBox` (compiler-created for mutable captures, auto-unwrapped). Distinguished by a bool flag on `HeapObject::LBox`.

5. **`Closure` has `location_map` and `doc`.** The `location_map: Rc<LocationMap>` field maps bytecode offsets to source locations for error reporting. The `doc: Option<Value>` field carries the docstring extracted from the function body, threaded from HIR through LIR.

6. **Tag encoding is transparent.** Callers use constructors and accessors; they don't manipulate tags directly.

7. **Floats are bit-exact.** `Value::float(f).as_float()` returns the exact same bits (including NaN, Infinity, -0.0).

8. **Integers are full-range i64.** The payload holds the complete i64 value.

## When to modify

- **Adding a new heap type**: Add variant to `HeapObject` enum in `value/heap.rs`, then add constructor and accessors here
- **Changing tag encoding**: Update tag constants and encoding/decoding logic
- **Adding new type predicates**: Add to `accessors.rs`
- **Changing Display/Debug format**: Update `traits.rs`

## Common pitfalls

- **Confusing nil and empty list**: Use `is_nil()` only for nil; use `is_empty_list()` for empty list
- **Assuming all floats are normal**: Handle NaN, Infinity, and -0.0 specially
- **Heap pointer alignment**: Heap pointers are stored in the payload u64
