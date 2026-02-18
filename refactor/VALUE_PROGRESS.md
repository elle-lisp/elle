# Value NaN-Boxing Refactor Progress

## Status: Foundation Complete

**Branch**: `value-nan-boxing`
**Last commit**: cb3fb1f

## Completed Work

### 1. Module Structure (`src/value/`)

```
src/value/
├── mod.rs           # Public API, re-exports from value_old for compatibility
├── repr.rs          # NaN-boxing representation (Value is 8 bytes, Copy)
├── heap.rs          # HeapObject enum for all heap-allocated types
├── closure.rs       # Unified Closure type (merges Closure/JitClosure)
├── condition.rs     # Condition system (existing, moved here)
├── display.rs       # Debug/Display impls (stub)
└── send.rs          # SendValue for threading (stub)
```

### 2. NaN-Boxing Encoding (`repr.rs`)

```
Value encoding (64-bit):
├── Floats:    Any f64 NOT in quiet NaN range
├── Nil:       0x7FFC_0000_0000_0000
├── False:     0x7FFC_0000_0000_0001
├── True:      0x7FFC_0000_0000_0002
├── Int:       0x7FF8_XXXX_XXXX_XXXX (48-bit signed, ±140 trillion)
├── Symbol:    0x7FF9_0000_XXXX_XXXX (32-bit ID)
├── Keyword:   0x7FFA_0000_XXXX_XXXX (32-bit ID)
└── Pointer:   0x7FFB_XXXX_XXXX_XXXX (48-bit heap address)
```

Key properties:
- `Value` is `Copy` (no more `.clone()` everywhere)
- 8 bytes exactly (down from 24)
- Fast type checks via bit manipulation
- All tests pass

### 3. HeapObject (`heap.rs`)

Single enum for all heap-allocated types:
- String, Cons, Vector, Table, Struct
- Closure, Condition, Coroutine
- Cell, Float (for NaN values)
- NativeFn, VmAwareFn
- LibHandle, CHandle, ThreadHandle

Unified cell type with `cell_mask` in Closure for auto-deref behavior.

### 4. Unified Closure (`closure.rs`)

Merges old Closure and JitClosure into single type:
- `bytecode: Option<Rc<Vec<u8>>>` - for interpretation
- `jit_code: Option<*const u8>` - for JIT execution
- `cell_mask: u64` - which captures auto-deref (replaces Cell/LocalCell distinction)
- `source_ast: Option<Rc<ClosureSource>>` - for deferred JIT

### 5. Value API

Constructors:
- `Value::NIL`, `Value::TRUE`, `Value::FALSE`
- `Value::int()`, `Value::float()`, `Value::bool()`
- `Value::symbol()`, `Value::keyword()`
- `Value::string()`, `Value::cons()`, `Value::vector()`
- `Value::table()`, `Value::closure()`, `Value::cell()`
- `list()` helper function

Predicates:
- `is_nil()`, `is_bool()`, `is_int()`, `is_float()`, `is_number()`
- `is_symbol()`, `is_keyword()`, `is_heap()`, `is_truthy()`
- `is_string()`, `is_cons()`, `is_vector()`, `is_table()`
- `is_closure()`, `is_cell()`, `is_list()`

Extractors:
- `as_bool()`, `as_int()`, `as_float()`, `as_number()`
- `as_symbol()`, `as_keyword()`
- `as_string()`, `as_cons()`, `as_vector()`
- `as_table()`, `as_closure()`, `as_cell()`
- `type_name()`, `list_to_vec()`

## Backward Compatibility

The old `src/value.rs` is preserved at `src/value_old/mod.rs`.
The new `src/value/mod.rs` re-exports all old types for compatibility.
The codebase continues to use old types until integration is complete.

## Remaining Work

### Phase 1.7: Remove Exception Variant ✅ COMPLETED
- Unified Exception into Condition system
- exception_id=0 reserved for generic exceptions
- Field 0 = message, Field 1 = data
- Added Condition::generic() and Condition::generic_with_data()
- Updated 48 call sites across 13 files
- All 2205 tests pass

### Phase 1.8: VM Integration
- Update all instruction handlers in `src/vm/*.rs`
- Change from `Value::Int(n)` pattern matching to `value.as_int()`
- Update stack operations for Copy semantics

### Phase 1.9: Primitives Integration
- Update ~100 functions in `src/primitives/*.rs`
- Change signatures to use new Value type
- Update pattern matching to accessor methods

### Phase 1.10: Test Suite
- Comprehensive integration tests
- Benchmark comparisons (before/after)
- Memory usage verification

## Testing Commands

```bash
# Run new value module tests
cargo test value::repr
cargo test value::heap
cargo test value::closure
cargo test value::condition

# Run all tests (should pass - new code is isolated)
cargo test
```

## Notes

- The new Value uses raw pointers for heap access - this is intentional
  for performance but requires careful lifetime management
- The `deref()` function returns `&'static HeapObject` which is technically
  incorrect lifetime but safe during migration
- JitClosure variant remains in HeapObject during transition
