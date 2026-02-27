# Elle Language Semantics

This document defines the authoritative semantics for Elle. When tests, code,
or documentation contradict this document, **this document is correct**.

## Truthiness

Elle has exactly two falsy values:

| Value | Truthy? | Representation |
|-------|---------|----------------|
| `false` | **No** | `Value::FALSE` |
| `nil` | **No** | `Value::NIL` |

**Everything else is truthy**, including:

| Value | Truthy? | Notes |
|-------|---------|-------|
| `()` | **Yes** | Empty list, distinct from nil |
| `0` | **Yes** | Zero is truthy (unlike C) |
| `0.0` | **Yes** | Float zero is truthy |
| `""` | **Yes** | Empty string is truthy |
| `[]` | **Yes** | Empty array is truthy |
| `true` | **Yes** | Boolean true |

### Why nil ≠ empty list

In Elle, `nil` and `()` (the empty list) are **distinct values**:

- **`nil`** represents the absence of a value. It is used for:
  - Functions that return "nothing" (like `display`)
  - Default/missing values
  - Logical false in conditions

- **`()`** represents an empty list. It is:
  - A valid list (just with no elements)
  - The terminator for proper lists
  - Truthy (because it IS a value, not the absence of one)

This matches Janet's design and modern Lisp conventions.

### Implementation

Truthiness is implemented in `src/value/repr.rs`:

```rust
pub fn is_truthy(&self) -> bool {
    self.0 != TAG_FALSE && self.0 != TAG_NIL
}
```

**DO NOT CHANGE THIS IMPLEMENTATION** without updating this document and all
dependent tests.

See `docs/types.md` for the complete type system reference.

## Lists

Lists are built from cons cells and terminate with `EMPTY_LIST`:

```
(1 2 3) = cons(1, cons(2, cons(3, EMPTY_LIST)))
```

**NOT** `cons(1, cons(2, cons(3, NIL)))`. The distinction matters for truthiness.

### List predicates

| Expression | Result | Notes |
|------------|--------|-------|
| `(nil? nil)` | `true` | Only nil is nil |
| `(nil? ())` | `false` | Empty list is NOT nil |
| `(empty? nil)` | error | Nil is not a container |
| `(empty? ())` | `true` | Empty list is empty |
| `(list? ())` | `true` | Empty list is a list |
| `(list? nil)` | `false` | Nil is not a list, it represents absence |
| `(pair? ())` | `false` | Empty list is not a pair |
| `(pair? nil)` | `false` | Nil is not a pair |

## Conditional Evaluation

The `if` special form evaluates the test expression and:
- If the result is **falsy** (`false` or `nil`), evaluates the else branch
- If the result is **truthy** (anything else), evaluates the then branch

```lisp
(if ()  "yes" "no")  # ⟹ "yes" (empty list is truthy)
(if nil "yes" "no")  # ⟹ "no"  (nil is falsy)
(if 0   "yes" "no")  # ⟹ "yes" (0 is truthy)
(if false  "yes" "no")  # ⟹ "no"  (false is falsy)
```

## Equality

`nil` and `()` are **not equal**:

```lisp
(= nil ())   # ⟹ false
(eq? nil ()) # ⟹ false
```

They have different NaN-boxed representations:
- `nil` = `0x7FFC_0000_0000_0000`
- `()` = `0x7FFC_0000_0000_0003`

## Destructuring

Destructuring in binding forms (`def`, `var`, `let`, `let*`, `fn` parameters)
uses **silent nil semantics**: it always succeeds at runtime.

### Rules

| Situation | Result |
|-----------|--------|
| Missing list element | `nil` |
| Missing array element | `nil` |
| Wrong type (e.g., destructure a number) | `nil` for all bindings |
| Extra elements | Silently ignored |
| `_` wildcard | Matches anything, no binding created |
| `& name` with no remaining elements | `()` for lists, `[]` for arrays |

### List rest vs Array rest

The `& name` rest pattern preserves the source type:
- List destructuring `(a & r)` produces a **list** for `r`
- Array destructuring `[a & r]` produces an **array** for `r`

When all fixed elements are consumed, the rest binding is:
- `()` (empty list) for list patterns — **truthy**
- `[]` (empty array) for array patterns — **truthy**
- Never `nil`

### Destructuring vs Pattern Matching

Destructuring (`def`, `let`, etc.) is **unconditional extraction** — it always
succeeds, binding `nil` for missing values. Pattern matching (`match`) is
**conditional** — it tests whether the value fits the pattern and branches.

These are separate systems. Destructuring patterns do not support literal
matching, guard clauses, or failure branches.

---

## Maintaining This Document

If you need to change these semantics:

1. Update this document FIRST
2. Update `src/value/repr.rs` to match
3. Update all tests to match
4. Update AGENTS.md files to match
5. Update docs/whats-new.md to match

**Never** change code to "fix" tests that contradict this document.
The tests are wrong if they contradict this document.
