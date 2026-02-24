# Elle Language Semantics

This document defines the authoritative semantics for Elle. When tests, code,
or documentation contradict this document, **this document is correct**.

## Truthiness

Elle has exactly two falsy values:

| Value | Truthy? | Representation |
|-------|---------|----------------|
| `#f` | **No** | `Value::FALSE` |
| `nil` | **No** | `Value::NIL` |

**Everything else is truthy**, including:

| Value | Truthy? | Notes |
|-------|---------|-------|
| `()` | **Yes** | Empty list, distinct from nil |
| `0` | **Yes** | Zero is truthy (unlike C) |
| `0.0` | **Yes** | Float zero is truthy |
| `""` | **Yes** | Empty string is truthy |
| `[]` | **Yes** | Empty array is truthy |
| `#t` | **Yes** | Boolean true |

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

## Lists

Lists are built from cons cells and terminate with `EMPTY_LIST`:

```
(1 2 3) = cons(1, cons(2, cons(3, EMPTY_LIST)))
```

**NOT** `cons(1, cons(2, cons(3, NIL)))`. The distinction matters for truthiness.

### List predicates

| Expression | Result | Notes |
|------------|--------|-------|
| `(nil? nil)` | `#t` | Only nil is nil |
| `(nil? ())` | `#f` | Empty list is NOT nil |
| `(empty? nil)` | error | Nil is not a container |
| `(empty? ())` | `#t` | Empty list is empty |
| `(list? ())` | `#t` | Empty list is a list |
| `(list? nil)` | `#f` | Nil is not a list, it represents absence |
| `(pair? ())` | `#f` | Empty list is not a pair |
| `(pair? nil)` | `#f` | Nil is not a pair |

## Conditional Evaluation

The `if` special form evaluates the test expression and:
- If the result is **falsy** (`#f` or `nil`), evaluates the else branch
- If the result is **truthy** (anything else), evaluates the then branch

```lisp
(if ()  "yes" "no")  ; ⟹ "yes" (empty list is truthy)
(if nil "yes" "no")  ; ⟹ "no"  (nil is falsy)
(if 0   "yes" "no")  ; ⟹ "yes" (0 is truthy)
(if #f  "yes" "no")  ; ⟹ "no"  (#f is falsy)
```

## Equality

`nil` and `()` are **not equal**:

```lisp
(= nil ())   ; ⟹ #f
(eq? nil ()) ; ⟹ #f
```

They have different NaN-boxed representations:
- `nil` = `0x7FFC_0000_0000_0000`
- `()` = `0x7FFC_0000_0000_0003`

---

## Maintaining This Document

If you need to change these semantics:

1. Update this document FIRST
2. Update `src/value/repr.rs` to match
3. Update all tests to match
4. Update AGENTS.md files to match
5. Update docs/WHAT_S_NEW.md to match

**Never** change code to "fix" tests that contradict this document.
The tests are wrong if they contradict this document.
