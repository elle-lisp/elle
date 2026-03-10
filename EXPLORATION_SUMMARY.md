# Elle Codebase Exploration Summary

## 1. prim_identical Function
**Location:** `src/primitives/comparison.rs`, lines 101-119

```rust
pub(crate) fn prim_identical(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("identical?: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    (
        SIG_OK,
        if args[0] == args[1] {
            Value::TRUE
        } else {
            Value::FALSE
        },
    )
}
```

**Key Points:**
- Strict identity comparison with NO numeric coercion
- `(identical? 1 1.0)` returns `false` (unlike `=` which returns `true`)
- Uses bitwise/structural equality via `==` operator on `Value`
- Registered as primitive with `Arity::Exact(2)`
- Aliases: none (only `identical?`)

---

## 2. prim_char_at Function
**Location:** `src/primitives/string.rs`, lines 165-221

```rust
pub(crate) fn prim_char_at(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("char-at: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let (s, _is_buffer) = match as_text(&args[0], "char-at") {
        Ok(v) => v,
        Err(e) => return e,
    };

    let index = match args[1].as_int() {
        Some(n) => n as usize,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("char-at: expected integer, got {}", args[1].type_name()),
                ),
            )
        }
    };
    let grapheme_count = s.graphemes(true).count();

    if index >= grapheme_count {
        return (
            SIG_ERROR,
            error_val(
                "error",
                format!(
                    "char-at: index {} out of bounds (length {})",
                    index, grapheme_count
                ),
            ),
        );
    }

    match s.graphemes(true).nth(index) {
        Some(g) => (SIG_OK, Value::string(g)),
        None => (
            SIG_ERROR,
            error_val(
                "error",
                format!(
                    "char-at: index {} out of bounds (length {})",
                    index, grapheme_count
                ),
            ),
        ),
    }
}
```

**Key Points:**
- Takes 2 arguments: string/buffer and integer index
- Returns single-character string at index
- Uses grapheme clusters (Unicode-aware)
- Registered as `string/char-at` with alias `char-at`
- Arity: `Exact(2)`
- Lint rule at `src/lint/rules.rs:141` expects arity 2

---

## 3. prim_take and prim_drop Functions
**Location:** `src/primitives/list/advanced.rs`

### prim_take (lines 470-509)
```rust
pub(crate) fn prim_take(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("take: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let count = match args[0].as_int() {
        Some(n) if n < 0 => {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("take: count must be non-negative, got {}", n),
                ),
            );
        }
        Some(n) => n as usize,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("take: expected integer, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let vec = match args[1].list_to_vec() {
        Ok(v) => v,
        Err(e) => return (SIG_ERROR, error_val("type-error", format!("take: {}", e))),
    };

    let taken: Vec<Value> = vec.into_iter().take(count).collect();
    (SIG_OK, list(taken))
}
```

### prim_drop (lines 512-551)
```rust
pub(crate) fn prim_drop(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("drop: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let count = match args[0].as_int() {
        Some(n) if n < 0 => {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("drop: count must be non-negative, got {}", n),
                ),
            );
        }
        Some(n) => n as usize,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("drop: expected integer, got {}", args[0].type_name()),
                ),
            )
        }
    };
    let vec = match args[1].list_to_vec() {
        Ok(v) => v,
        Err(e) => return (SIG_ERROR, error_val("type-error", format!("drop: {}", e))),
    };

    let dropped: Vec<Value> = vec.into_iter().skip(count).collect();
    (SIG_OK, list(dropped))
}
```

**Key Points:**
- Both take 2 arguments: count (integer) and list
- `take`: returns first N elements
- `drop`: returns all elements after first N
- Both require non-negative count
- Both return lists (via `list()` helper)
- Registered in `src/primitives/list/mod.rs`

---

## 4. prim_sort Function
**Location:** `src/primitives/sort.rs`, lines 13-123

**Key Points:**
- Takes 1 argument: collection to sort
- Type-preserving: arrays mutated in place, lists/tuples return new values
- All elements must be numbers
- Uses `total_cmp()` for proper NaN handling
- Registered with `Arity::Exact(1)`

---

## 5. defn Macro Definition
**Location:** `prelude.lisp`, lines 9-10

```lisp
(defmacro defn (name params & body)
  `(def ,name (fn ,params ,;body)))
```

**Key Points:**
- Macro (not a special form)
- Desugars to `(def name (fn params body...))`
- Loaded in prelude before user code expansion
- Supports destructuring in params (handled by `fn`)
- Supports variadic params with `&`
- Supports optional params with `&opt`
- Supports keyword params with `&keys`
- Supports named params with `&named`

---

## 6. Lint Rules for char-at
**Location:** `src/lint/rules.rs`, line 141

```rust
"char-at" => Some(2),
```

**Key Points:**
- Arity check expects exactly 2 arguments
- Part of `builtin_arity()` function
- Used by `check_call_arity()` for linting

---

## 7. Primitive Registration Pattern
**Location:** `src/primitives/mod.rs`

Module structure:
```rust
pub mod access;
pub mod allocator;
pub mod arena;
pub mod arithmetic;
// ... many more modules
pub mod string;
pub mod structs;
pub mod time;
pub mod types;
pub mod unix;
```

**Key Points:**
- Each module contains primitives for a domain
- Each module exports a `PRIMITIVES: &[PrimitiveDef]` array
- `registration.rs` calls `register_primitives()` to install all
- `PrimitiveDef` struct contains: name, func, effect, arity, doc, params, category, example, aliases

---

## 8. Arithmetic Primitives
**Location:** `src/primitives/arithmetic.rs`

Key functions:
- `prim_add` (lines 9-35): Variadic `+`, identity 0
- `prim_sub` (lines 38-61): Variadic `-`, unary negation
- `prim_mul` (lines 64-90): Variadic `*`, identity 1
- `prim_mod` (lines 92-108): Euclidean modulo (result sign matches divisor)
- `prim_rem` (lines 110-126): Truncated remainder (result sign matches dividend)
- `prim_div_vm` (lines 240-285): Variadic `/`, checks for division by zero

**Key Points:**
- All use shared `arithmetic::` module functions
- Return `(SIG_ERROR, error_val(...))` on type/arity errors
- Variadic operations support chaining
- `mod` and `rem` differ in sign behavior

---

## 9. Value Type and NaN-Boxing
**Location:** `src/value/repr/mod.rs`

### NaN-Boxing Encoding

IEEE 754 double-precision: 1 sign + 11 exponent + 52 mantissa = 64 bits

Upper 16 bits as type tags, lower 48 bits as payload:

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

### Key Constants
```rust
pub const INT_MAX: i64 = 0x7FFF_FFFF_FFFF;  // 2^47 - 1
pub const INT_MIN: i64 = -0x8000_0000_0000; // -2^47
```

### Value Struct
```rust
#[derive(Clone, Copy)]
#[repr(transparent)]
pub struct Value(pub(crate) u64);
```

**Key Points:**
- `Value` is `Copy` (8 bytes fit in register)
- Heap data is `Rc`
- `nil` ≠ empty list (both falsy/truthy differ)
- Comparison uses `==` operator (bitwise equality)
- `to_bits()` and `from_bits()` for raw access

---

## 10. Integer Conversion Function
**Location:** `src/primitives/convert.rs`, lines 9-50

**Registered as:**
- `integer` (primary name)
- `int` (alias)
- `string->integer` (Scheme-style)
- `string->int` (Scheme-style alias)

**Key Points:**
- Accepts int, float, or string
- Float truncates to integer
- String is parsed as i64
- Returns 48-bit signed integer

---

## 11. stdlib.lisp and prelude.lisp Usage

### prelude.lisp (lines 1-100)
- Macro definitions only (no runtime code)
- Loaded before user code expansion
- Contains: `defn`, `let*`, `->`, `->>`, `when`, `unless`, `error`, `try/catch`, `protect`, `defer`, `with`

### stdlib.lisp (lines 1-100)
- Runtime function definitions
- Loaded after primitives registered
- Contains: `map`, `filter`, `fold`, `reduce`, `keep`, `identity`, `complement`, `constantly`, `compose`, `comp`, `partial`, `juxt`, `all?`

**Usage of char-at in stdlib.lisp (line 21):**
```lisp
(loop (+ i 1) (cons (f (string/char-at coll i)) acc))
```

---

## 12. File Locations Summary

| File | Purpose | Key Content |
|------|---------|-------------|
| `src/primitives/comparison.rs` | Comparison primitives | `prim_identical`, `prim_eq`, `prim_lt`, etc. |
| `src/primitives/string.rs` | String primitives | `prim_char_at`, `prim_string_upcase`, etc. |
| `src/primitives/list/advanced.rs` | List operations | `prim_take`, `prim_drop`, `prim_append`, `prim_reverse` |
| `src/primitives/sort.rs` | Sort and range | `prim_sort`, `prim_range` |
| `src/primitives/arithmetic.rs` | Arithmetic | `prim_add`, `prim_sub`, `prim_mul`, `prim_mod`, `prim_rem` |
| `src/primitives/convert.rs` | Type conversion | `prim_to_int`, `prim_to_float`, `prim_to_string` |
| `src/primitives/types.rs` | Type checking | `prim_is_integer`, `prim_is_string`, `prim_type_of` |
| `src/primitives/mod.rs` | Module exports | Re-exports all primitive modules |
| `src/lint/rules.rs` | Lint rules | `builtin_arity()`, `check_naming_convention()` |
| `src/value/repr/mod.rs` | NaN-boxing | `Value` type, tag constants, encoding |
| `src/value/repr/constructors.rs` | Value constructors | `Value::int()`, `Value::string()`, etc. |
| `src/value/repr/accessors.rs` | Value accessors | `as_int()`, `as_string()`, type checks |
| `src/syntax/mod.rs` | Syntax tree | `Syntax`, `SyntaxKind` types |
| `src/syntax/expand/mod.rs` | Macro expansion | `Expander` struct, macro handling |
| `prelude.lisp` | Prelude macros | `defn`, `let*`, `->`, `->>`, etc. |
| `stdlib.lisp` | Standard library | `map`, `filter`, `fold`, etc. |

---

## 13. Primitive Registration Flow

1. **Module definition** (e.g., `src/primitives/string.rs`)
   - Define function: `pub(crate) fn prim_char_at(...)`
   - Define `PRIMITIVES: &[PrimitiveDef]` array

2. **Module export** (`src/primitives/mod.rs`)
   - `pub mod string;`

3. **Registration** (`src/primitives/registration.rs`)
   - Calls `register_primitives(vm, symbols)`
   - Installs all primitives into VM

4. **Usage**
   - Primitives available as global functions
   - Called via `vm/call.rs` dispatch

---

## 14. Key Invariants

1. **Primitives validate arguments** - Return `(SIG_ERROR, error_val(...))` for arity/type errors
2. **All primitives return `(SignalBits, Value)`** - No exceptions
3. **No primitive has VM access** - Operations needing VM return `SIG_RESUME`
4. **`nil` ≠ empty list** - `nil` is falsy (absence), `()` is truthy (empty list)
5. **`identical?` uses bitwise equality** - No numeric coercion
6. **`=` uses numeric-aware equality** - `(= 1 1.0)` is true
7. **Collections are type-preserving** - `sort` mutates arrays, returns new lists/tuples
8. **Grapheme-aware string operations** - `char-at` uses Unicode grapheme clusters

---

## 15. Search Results Summary

### Files containing "char-at" or "char_at"
- `src/primitives/string.rs` - Definition and registration
- `src/lint/rules.rs` - Arity check
- `stdlib.lisp` - Usage in `map` function
- `tests/elle/strings.lisp` - Tests

### Files containing "prim_take", "prim_drop", "prim_sort"
- `src/primitives/list/advanced.rs` - `prim_take`, `prim_drop`
- `src/primitives/sort.rs` - `prim_sort`
- `src/primitives/list/mod.rs` - Registration

### Files containing "prim_identical"
- `src/primitives/comparison.rs` - Definition and registration

### Files containing "defn" macro
- `prelude.lisp` - Definition (line 9)
- Multiple test files - Usage

### Files containing "integer" conversion
- `src/primitives/convert.rs` - `prim_to_int` (registered as `integer`)
- `src/primitives/types.rs` - `prim_is_integer` (type check)
