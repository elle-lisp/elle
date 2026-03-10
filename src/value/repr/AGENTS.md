# value/repr

NaN-boxing representation: Value constructors, accessors, and trait implementations.

## Responsibility

- Define the `Value` type (NaN-boxed 8-byte representation)
- Provide constructors for immediate and heap-allocated values
- Provide accessors and type checking predicates
- Implement `Display`, `Debug`, `Clone`, `PartialEq` for values
- Handle NaN-boxing encoding/decoding

Does NOT:
- Allocate heap objects (that's `value/heap`)
- Execute code (that's `vm`)
- Manage fibers (that's `value/fiber`)

## Key types

| Type | Purpose |
|------|---------|
| `Value` | NaN-boxed 8-byte value (Copy) |

## NaN-boxing encoding

IEEE 754 double-precision: 1 sign + 11 exponent + 52 mantissa = 64 bits

A quiet NaN has: exponent = all 1s (0x7FF), mantissa bit 51 = 1
This gives us the quiet NaN prefix: 0x7FF8 in the upper 16 bits

Our encoding uses upper 16 bits as type tags, lower 48 bits as payload:

| Tag | Upper 16 bits | Payload | Type |
|-----|---------------|---------|------|
| Float | Not 0x7FF8+ | 64-bit float bits | Any f64 that is NOT a quiet NaN |
| Int | 0x7FF8 | 48-bit signed integer | Integer (-2^47 to 2^47-1) |
| Falsy | 0x7FF9 | 0 (nil) or 1 (false) | Nil or False |
| EmptyList | 0x7FFA | (none) | Empty list (truthy) |
| Pointer | 0x7FFB | 48-bit heap pointer | Cons, Array, Table, Closure, Fiber, etc. |
| Truthy | 0x7FFC | Bit 47=0: singleton (0=true, 1=undefined), Bit 47=1: symbol (32-bit ID) | True, Undefined, or Symbol |
| NaN/Inf | 0x7FFD | 64-bit float bits | NaN or Infinity |
| PtrVal | 0x7FFE | Bit 47=0: keyword (47-bit ptr), Bit 47=1: cpointer (47-bit ptr) | Keyword or C pointer |
| SSO | 0x7FFF | Up to 6 UTF-8 bytes | Short string (reserved) |

## Files

| File | Lines | Content |
|------|-------|---------|
| `mod.rs` | ~280 | NaN-boxed Value type, tag encoding, constants |
| `constructors.rs` | ~380 | Value construction methods (int, float, bool, symbol, keyword, cons, array, table, closure, fiber, etc.) |
| `accessors.rs` | ~670 | Value field access and type checking (is_int, as_int, is_string, as_string, etc.) |
| `traits.rs` | ~150 | Display, Debug, Clone implementations |
| `tests.rs` | ~100 | NaN-boxing roundtrip tests |

## Constructors

| Constructor | Type | Notes |
|-------------|------|-------|
| `Value::int(n)` | Int | Panics if outside 48-bit range |
| `Value::float(f)` | Float | Handles NaN/Infinity specially |
| `Value::bool(b)` | Bool | True or False |
| `Value::symbol(id)` | Symbol | From SymbolId |
| `Value::keyword(name)` | Keyword | Interned string |
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
| `Value::closure(c)` | Closure | Bytecode + env + arity + effect + location_map |
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
| `as_keyword()` | Option<&str> | Extract keyword name |
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

1. **`Value` is `Copy`.** All 8 bytes fit in a register. Heap data is `Rc`.

2. **`nil` ≠ empty list.** `Value::NIL` is falsy (absence). `Value::EMPTY_LIST` is truthy (empty list). Lists terminate with `EMPTY_LIST`, not `NIL`.

3. **Two lbox types exist.** `LBox` (user-created via `box`, explicit deref) and `LocalLBox` (compiler-created for mutable captures, auto-unwrapped). Distinguished by a bool flag on `HeapObject::LBox`.

4. **`Closure` has `location_map` and `doc`.** The `location_map: Rc<LocationMap>` field maps bytecode offsets to source locations for error reporting. The `doc: Option<Value>` field carries the docstring extracted from the function body, threaded from HIR through LIR.

5. **NaN-boxing is transparent.** Callers use constructors and accessors; they don't manipulate bits directly.

6. **Floats are bit-exact.** `Value::float(f).as_float()` returns the exact same bits (including NaN, Infinity, -0.0).

7. **Integers are sign-extended.** 48-bit signed integers are stored with sign extension in the lower 48 bits.

## When to modify

- **Adding a new heap type**: Add variant to `HeapObject` enum in `value/heap.rs`, then add constructor and accessors here
- **Changing NaN-boxing encoding**: Update tag constants and encoding/decoding logic
- **Adding new type predicates**: Add to `accessors.rs`
- **Changing Display/Debug format**: Update `traits.rs`

## Common pitfalls

- **Confusing nil and empty list**: Use `is_nil()` only for nil; use `is_empty_list()` for empty list
- **Assuming all floats are normal**: Handle NaN, Infinity, and -0.0 specially
- **Integer overflow**: Check bounds before calling `Value::int()` (panics on overflow)
- **Heap pointer alignment**: Heap pointers must fit in 48 bits (2^48 address space)
