# Complete Inventory of Generic "error" Calls in Elle Codebase

**Generated:** 2026-03-19  
**Location:** `/home/adavidoff/git/tmp/elle-issue-556/src`  
**Total Matches:** 70 calls across 22 files

---

## Executive Summary

This inventory identifies all calls using the generic `"error"` keyword instead of specific error types like `"type-error"`, `"io-error"`, `"arity-error"`, etc.

### Key Findings

| Metric | Value |
|--------|-------|
| **Total generic errors** | 70 |
| **Rust source files** | 21 |
| **Elle source files** | 1 |
| **error_val() calls** | 62 |
| **set_error() calls** | 7 |
| **{:error :error} structs** | 1 |

---

## Distribution by File

### Top 5 Files with Most Generic Errors

1. **src/primitives/list/mod.rs** — 12 calls
   - Collection introspection (length, empty?)
   
2. **src/primitives/array.rs** — 8 calls
   - Mutable collection operations (pop, insert, remove)
   
3. **src/vm/fiber.rs** — 8 calls
   - Fiber signal handling (resume, abort, propagate)
   
4. **src/primitives/arithmetic.rs** — 7 calls
   - Arithmetic operations (add, sub, mul, mod, etc.)
   
5. **src/primitives/fiber_introspect.rs** — 5 calls
   - Fiber introspection/control (cancel, abort)

### Complete File List

```
src/primitives/list/mod.rs              12 calls
src/primitives/array.rs                  8 calls
src/vm/fiber.rs                          8 calls
src/primitives/arithmetic.rs             7 calls
src/primitives/fiber_introspect.rs       5 calls
src/primitives/modules.rs                3 calls
src/primitives/path.rs                   3 calls
src/primitives/json/mod.rs               3 calls
src/primitives/concurrency.rs            3 calls
src/primitives/convert.rs                3 calls
src/jit/calls.rs                         2 calls
src/primitives/time.rs                   2 calls
src/primitives/fileio.rs                 2 calls
src/primitives/list/advanced.rs          1 call
src/vm/call.rs                           1 call
src/vm/signal.rs                         1 call
src/primitives/meta.rs                   1 call
src/primitives/sort.rs                   1 call
src/primitives/bitwise.rs                1 call
src/primitives/chan.rs                   1 call
src/primitives/fibers.rs                 1 call
stdlib.lisp                              1 call
────────────────────────────────────────────────
TOTAL                                   70 calls
```

---

## Distribution by Error Category

| Category | Count | Primary Files |
|----------|-------|----------------|
| Collection operations | 13 | list/mod.rs, list/advanced.rs |
| Fiber operations | 11 | fiber.rs, fiber_introspect.rs, fibers.rs |
| Arithmetic/conversion | 10 | arithmetic.rs, convert.rs |
| Stack overflow | 3 | call.rs, jit/calls.rs |
| Path operations | 3 | path.rs |
| Module/import | 3 | modules.rs |
| JSON | 3 | json/mod.rs |
| Concurrency | 3 | concurrency.rs |
| Time/clock | 2 | time.rs |
| File I/O | 2 | fileio.rs |
| Other | 16 | signal.rs, meta.rs, sort.rs, bitwise.rs, chan.rs, stdlib.lisp |

---

## Pattern Analysis

### Pattern 1: Direct error_val() calls (62 calls)

**Format:** `error_val("error", message)`

**Examples:**
```rust
error_val("error", "fiber/propagate: no signal")
error_val("error", format!("path/absolute: {}", e))
error_val("error", "pop: empty array")
```

**Distribution:**
- Single-line calls: 62
- Multi-line calls: 0

### Pattern 2: set_error() wrapper calls (7 calls)

**Format:** `set_error(&mut fiber, "error", message)`

**Files:**
- src/vm/fiber.rs (6 calls)
- src/vm/call.rs (1 call)

**Examples:**
```rust
set_error(&mut self.fiber, "error", "SIG_RESUME with non-fiber value");
set_error(&mut self.fiber, "error", "Stack overflow");
```

### Pattern 3: Elle-side error structs (1 call)

**Format:** `{:error :error :message "..."}`

**Location:** stdlib.lisp:1081

**Example:**
```lisp
(error {:error :error :message "ev/shutdown: not inside an event loop"})
```

---

## Detailed Call Sites

### Fiber Operations (11 calls)

**src/vm/fiber.rs:**
- Line 164: `set_error` — SIG_RESUME with non-fiber value
- Line 258: `set_error` — SIG_RESUME with non-fiber value
- Line 470: `error_val` — fiber/propagate: no signal
- Line 509: `error_val` — fiber/propagate: no signal
- Line 548: `set_error` — SIG_ABORT with non-fiber value
- Line 594: `set_error` — SIG_ABORT with non-fiber value
- Line 647: `set_error` — SIG_RESUME with non-fiber value
- Line 709: `error_val` — fiber/propagate: no signal

**src/primitives/fiber_introspect.rs:**
- Line 278: `error_val` — fiber/cancel: cannot cancel a completed fiber
- Line 282: `error_val` — fiber/cancel: fiber already errored
- Line 341: `error_val` — fiber/abort: cannot abort a running fiber
- Line 345: `error_val` — fiber/abort: cannot abort a completed fiber
- Line 349: `error_val` — fiber/abort: fiber already errored

**src/primitives/fibers.rs:**
- Line 249: `error_val` — fiber/resume: fiber is already running

### Collection Operations (13 calls)

**src/primitives/list/mod.rs (11 calls):**
- Lines 268, 279, 290, 316, 327: `length` operation failures
- Lines 393, 408, 419, 430, 441, 452: `empty?` operation failures

**src/primitives/list/advanced.rs (1 call):**
- Line 588: `last` — cannot get last of empty list

**src/primitives/array.rs (8 calls):**
- Line 39: `array/new` — size must be non-negative
- Lines 163, 175, 210: `pop` — empty collection
- Line 245: `popn` — count must be non-negative
- Lines 308, 399, 421: `insert`/`remove` — index/count validation

### Arithmetic Operations (7 calls)

**src/primitives/arithmetic.rs:**
- Line 31: Addition error
- Line 49: Negation error
- Line 57: Subtraction error
- Line 86: Multiplication error
- Line 106: Modulo error
- Line 124: Remainder error
- Line 140: Absolute value error

### Type Conversion (3 calls)

**src/primitives/convert.rs:**
- Line 96: `float` — cannot parse as float
- Line 263: `string` — invalid UTF-8 (bytes)
- Line 275: `string` — invalid UTF-8 (@bytes)

### Stack Overflow (3 calls)

**src/vm/call.rs:**
- Line 182: `set_error` — Stack overflow

**src/jit/calls.rs:**
- Line 253: `error_val` — Stack overflow
- Line 377: `error_val` — Stack overflow

### Path Operations (3 calls)

**src/primitives/path.rs:**
- Line 142: `path/absolute` error
- Line 156: `path/canonicalize` error
- Line 222: `path/cwd` error

### Module/Import (3 calls)

**src/primitives/modules.rs:**
- Line 39: `import` — VM context not initialized
- Line 81: `import` — plugin loading error
- Line 91: `import` — failed to read file

### JSON (3 calls)

**src/primitives/json/mod.rs:**
- Line 74: JSON parse error
- Line 92: JSON serialize error
- Line 111: JSON serialize-pretty error

### Concurrency (3 calls)

**src/primitives/concurrency.rs:**
- Line 111: Thread spawn error
- Line 160: Thread join error
- Line 169: `join` — thread did not complete in time

### Time/Clock (2 calls)

**src/primitives/time.rs:**
- Line 57: `clock/cpu` — clock_gettime failed
- Line 158: `ev/sleep` — duration must be non-negative

### File I/O (2 calls)

**src/primitives/fileio.rs:**
- Line 27: `slurp` — failed to read file
- Line 82: `spit` — failed to write file

### Other (6 calls)

**src/vm/signal.rs:**
- Line 222: `SIG_QUERY` — expected cons cell

**src/primitives/meta.rs:**
- Line 50: `gensym` — symbol table not available

**src/primitives/sort.rs:**
- Line 163: `range` — step cannot be zero

**src/primitives/bitwise.rs:**
- Line 171: `bit/shift-left` — shift amount must be non-negative

**src/primitives/chan.rs:**
- Line 217: `chan/clone` — sender is closed

**stdlib.lisp:**
- Line 1081: `ev/shutdown` — not inside an event loop

---

## Data Files

Two additional files have been generated for further analysis:

1. **ERROR_INVENTORY.csv** — Machine-readable CSV format with columns:
   - file, line, pattern, error_message, category

2. **ERROR_INVENTORY_SUMMARY.md** — This document

---

## Notes

1. **No multi-line patterns:** All `error_val("error", ...)` calls are on single lines.
2. **Consistent usage:** All 70 calls use the generic `"error"` keyword.
3. **No false positives:** Calls using specific keywords (type-error, io-error, etc.) were excluded.
4. **Complete coverage:** All Rust source files in src/ and stdlib.lisp were searched.

---

## Recommendations

For issue #556, these 70 call sites should be reviewed to determine if they should use more specific error keywords. The hotspots are:

1. **list/mod.rs** (12 calls) — Consider `"type-error"` for collection type mismatches
2. **array.rs** (8 calls) — Consider `"type-error"` or `"value-error"` for validation failures
3. **fiber.rs** (8 calls) — Consider `"type-error"` for non-fiber values
4. **arithmetic.rs** (7 calls) — Consider `"type-error"` for type mismatches

---

**End of Inventory**
