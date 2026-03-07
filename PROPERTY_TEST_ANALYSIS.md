# Property Test Categorization Analysis

**Total lines analyzed**: 3,496 lines across 11 property test files

---

## Summary

**Category A (True Property Tests)**: 120 tests
- These genuinely need random input generation to verify algebraic invariants
- Should remain as proptest

**Category B (Behavioral Tests in Disguise)**: 42 tests
- Generate random inputs but test "does this expression produce the expected value?"
- Can be rewritten as Elle test scripts with fixed representative inputs
- Don't need random generation — they're testing behavior, not invariants

---

## CATEGORY A: TRUE PROPERTY TESTS (Keep as proptest)

### arithmetic.rs (2 tests)
1. **`int_plus_float_is_float`** — Verifies type promotion law: int + float → float
   - Tests algebraic invariant across 200 random (int, float) pairs
   - **Category A**

2. **`float_plus_int_is_float`** — Verifies commutativity of type promotion
   - Tests that float + int also produces float
   - **Category A**

### comparison.rs (4 tests)
1. **`string_lt_ge_complementary`** — Verifies < and >= are complementary
   - Tests logical invariant: `(< a b) != (>= a b)` for all string pairs
   - **Category A**

2. **`string_gt_le_complementary`** — Verifies > and <= are complementary
   - Tests logical invariant: `(> a b) != (<= a b)`
   - **Category A**

3. **`string_lt_transitive`** — Verifies transitivity of <
   - Tests mathematical law: if a < b and b < c, then a < c
   - **Category A**

4. **`string_le_is_lt_or_eq`** — Verifies definition of <=
   - Tests algebraic equivalence: `(<= a b) ≡ (< a b) ∨ (= a b)`
   - **Category A**

### effects.rs (13 tests)
All tests verify algebraic laws on the Effect type:
- `effect_combine_commutative` — Verifies combine is commutative
- `effect_combine_associative` — Verifies combine is associative
- `effect_combine_identity` — Verifies Effect::none() is identity
- `effect_combine_idempotent` — Verifies combine is idempotent
- `effect_propagates_combine` — Verifies propagates field is ORed
- `polymorphic_effect_is_polymorphic` — Verifies polymorphic effect marking
- `polymorphic_propagates_correct_param` — Verifies param propagation
- `polymorphic_errors_has_error_bit` — Verifies error bit in polymorphic_errors
- `none_effect_is_not_yielding` — Verifies Effect::none() properties
- `yields_effect_may_yield` — Verifies yields effect has yield bit
- `errors_effect_may_error` — Verifies errors effect has error bit
- `yields_errors_has_both` — Verifies combined effect
- `ffi_effect_may_ffi` — Verifies FFI effect
- `halts_effect_may_halt` — Verifies halt effect

All **Category A** — algebraic laws and type invariants.

### path.rs (13 tests)
All tests verify algebraic properties of path operations:
- `join_then_parent_recovers_prefix` — Roundtrip: parent(join([p, c])) == p
- `join_then_filename_recovers_last` — Roundtrip: filename(join([p, c])) == c
- `with_extension_then_extension_roundtrips` — Roundtrip: extension(with_extension(b, e)) == e
- `with_extension_preserves_stem` — Invariant: stem unchanged by with_extension
- `normalize_idempotent` — Verifies idempotence: normalize(normalize(p)) == normalize(p)
- `absolute_relative_complementary` — Verifies is_absolute and is_relative are complementary
- `components_join_roundtrip` — Roundtrip: join(components(p)) == normalize(p)
- `relative_join_roundtrip` — Roundtrip: join([base, relative(target, base)]) == normalize(target)
- `join_absolute_replaces` — Invariant: join([rel, abs]) == abs
- `parent_is_shorter_or_empty` — Invariant: parent(p) is shorter than p
- `stem_always_some_for_filename` — Invariant: stem(filename) is always Some
- `filename_extension_roundtrip` — Roundtrip: stem and extension preserved

All **Category A** — roundtrip fidelity and algebraic properties.

### reader.rs (17 tests)
All tests verify the fundamental roundtrip property:
- `integer_roundtrip` — Verifies: read(display(read(n))) == read(n)
- `bool_roundtrip` — Verifies: read(display(read(b))) == read(b)
- `string_roundtrip` — Verifies: read(display(read(s))) == read(s)
- `symbol_roundtrip` — Verifies: read(display(read(sym))) == read(sym)
- `keyword_roundtrip` — Verifies: read(display(read(kw))) == read(kw)
- `float_roundtrip` — Verifies: read(display(read(f))) == read(f)
- `list_roundtrip` — Verifies: read(display(read(list))) == read(list)
- `reader_never_panics` — Verifies: reader never panics on arbitrary ASCII input
- `reader_never_panics_with_delimiters` — Verifies: reader never panics with delimiters
- `nil_roundtrip` — Verifies: read(display(read(nil))) == read(nil)
- `empty_list_roundtrip` — Verifies: read(display(read(()))) == read(())
- `empty_array_roundtrip` — Verifies: read(display(read(@[]))) == read(@[])
- `nested_list_roundtrip` — Verifies: read(display(read(nested))) == read(nested)
- `quoted_roundtrip` — Verifies: read(display(read('x))) == read('x)
- `multi_element_list_roundtrip` — Verifies: read(display(read(list))) == read(list)
- `array_roundtrip` — Verifies: read(display(read(@[...]))) == read(@[...])
- `table_roundtrip` — Verifies: read(display(read(@{...}))) == read(@{...})

All **Category A** — roundtrip fidelity invariants.

### nanboxing.rs (26 tests)
All tests verify NaN-boxing invariants:
- `int_roundtrip` — Verifies: Value::int(n).as_int() == Some(n)
- `int_is_int` — Verifies: exactly one type predicate true for int
- `float_roundtrip_normal` — Verifies: Value::float(f).as_float() == Some(f)
- `float_roundtrip_special` — Verifies: roundtrip for special floats (inf, nan, etc.)
- `float_is_float` — Verifies: exactly one type predicate true for float
- `symbol_roundtrip` — Verifies: Value::symbol(id).as_symbol() == Some(id)
- `symbol_is_symbol` — Verifies: exactly one type predicate true for symbol
- `bool_roundtrip` — Verifies: Value::bool(b).as_bool() == Some(b)
- `exactly_one_type_for_int` — Verifies: exactly one type predicate true for int
- `exactly_one_type_for_float` — Verifies: exactly one type predicate true for float
- `exactly_one_type_for_symbol` — Verifies: exactly one type predicate true for symbol
- `int_is_truthy` — Verifies: all ints are truthy
- `float_is_truthy` — Verifies: all floats are truthy
- `symbol_is_truthy` — Verifies: all symbols are truthy
- `string_is_truthy` — Verifies: all strings are truthy
- `int_eq_reflexive` — Verifies: v == v for all ints
- `float_eq_reflexive` — Verifies: v == v for all non-NaN floats
- `symbol_eq_reflexive` — Verifies: v == v for all symbols
- `int_eq_same_value` — Verifies: Value::int(n) == Value::int(n)
- `int_neq_different_value` — Verifies: Value::int(a) != Value::int(b) when a != b
- `bool_eq_same_value` — Verifies: Value::bool(b) == Value::bool(b)
- `int_not_eq_float` — Verifies: int and float have different bit patterns
- `cons_roundtrip` — Verifies: cons(car, cdr) roundtrips correctly
- `string_roundtrip` — Verifies: Value::string(s) roundtrips correctly
- `list_roundtrip` — Verifies: list construction and extraction roundtrips
- `array_roundtrip` — Verifies: array construction and extraction roundtrips

All **Category A** — roundtrip fidelity and type discrimination invariants.

### ffi.rs (45 tests)
All tests verify FFI invariants:
- Pointer roundtrip and type discrimination (5 tests)
- Marshal range checking for all integer types (15 tests)
- Memory read-write roundtrips (5 tests)
- TypeDesc size/align consistency (3 tests)
- String marshalling edge cases (3 tests)
- Struct marshalling roundtrip (1 test)
- Struct field count validation (2 tests)
- TypeDesc struct layout properties (4 tests)
- Array type properties (2 tests)
- FFIType value properties (5 tests)

All **Category A** — roundtrip fidelity, range validation, and type invariants.

---

## CATEGORY B: BEHAVIORAL TESTS IN DISGUISE (Migrate to Elle)

### matching.rs (4 tests)
1. **`match_wildcard_catches_all`** — Tests: wildcard pattern matches any value
   - Generates random int, tests `(match n (_ :caught))` == `:caught`
   - **Category B** → Elle equivalent:
   ```lisp
   (assert-eq (match 42 (_ :caught)) :caught)
   (assert-eq (match -1000 (_ :caught)) :caught)
   ```

2. **`match_result_in_call`** — Tests: match result can be used in call position
   - Generates random int n, tests `(+ 1 (match n (n n) (_ 0)))` == `n + 1`
   - **Category B** → Elle equivalent:
   ```lisp
   (assert-eq (+ 1 (match 42 (42 42) (_ 0))) 43)
   ```

3. **`match_guard_sees_binding`** — Tests: guard expressions see pattern bindings
   - Generates random int n, tests guard sees n
   - **Category B** → Elle equivalent:
   ```lisp
   (assert-eq (match 5 (x when (> x 0) :pos) (_ :zero)) :pos)
   (assert-eq (match -3 (x when (< x 0) :neg) (_ :zero)) :neg)
   ```

4. **`match_or_pattern_membership`** — Tests: or-patterns match alternatives
   - Generates random int 0-9, tests `(match n ((1|3|5|7|9) :odd) ...)`
   - **Category B** → Elle equivalent:
   ```lisp
   (assert-eq (match 1 ((1 | 3 | 5 | 7 | 9) :odd) (_ :even)) :odd)
   (assert-eq (match 2 ((1 | 3 | 5 | 7 | 9) :odd) (_ :even)) :even)
   ```

### strings.rs (16 tests)
1. **`slice_start_end_order`** — Tests: slice with valid range succeeds
   - **Category B** → Elle equivalent:
   ```lisp
   (assert-eq (string/slice "hello" 0 2) "he")
   ```

2. **`slice_oob_end_returns_nil`** — Tests: OOB end index returns nil
   - **Category B** → Elle equivalent:
   ```lisp
   (assert-eq (string/slice "hello" 0 100) nil)
   ```

3. **`slice_oob_start_returns_nil`** — Tests: OOB start index returns nil
   - **Category B** → Elle equivalent:
   ```lisp
   (assert-eq (string/slice "hello" 100 101) nil)
   ```

4. **`slice_reversed_range_returns_nil`** — Tests: reversed range returns nil
   - **Category B** → Elle equivalent:
   ```lisp
   (assert-eq (string/slice "hello" 3 1) nil)
   ```

5. **`slice_empty_string_oob_returns_nil`** — Tests: empty string OOB returns nil
   - **Category B** → Elle equivalent:
   ```lisp
   (assert-eq (string/slice "" 0 1) nil)
   ```

6. **`split_join_roundtrip`** — Tests: split/join roundtrip
   - **Category B** → Elle equivalent:
   ```lisp
   (assert-eq (string/join (string/split "a,b,c" ",") ",") "a,b,c")
   ```

7. **`split_produces_list`** — Tests: split produces a list
   - **Category B** → Elle equivalent:
   ```lisp
   (assert-true (or (cons? (string/split "a,b" ",")) (empty? (string/split "a,b" ","))))
   ```

8. **`number_to_string_roundtrip`** — Tests: number→string→number roundtrip
   - **Category B** → Elle equivalent:
   ```lisp
   (assert-eq (string->integer (number->string 42)) 42)
   ```

9. **`string_to_integer_roundtrip`** — Tests: string→integer roundtrip
   - **Category B** → Elle equivalent:
   ```lisp
   (assert-eq (string->integer "42") 42)
   ```

10. **`string_to_integer_invalid_returns_error`** — Tests: non-numeric string errors
    - **Category B** → Elle equivalent:
    ```lisp
    (assert-true (error? (string->integer "abc")))
    ```

11. **`char_at_valid_index`** — Tests: char-at returns single character
    - **Category B** → Elle equivalent:
    ```lisp
    (assert-eq (string/char-at "hello" 0) "h")
    ```

12. **`char_at_out_of_bounds_errors`** — Tests: char-at OOB errors
    - **Category B** → Elle equivalent:
    ```lisp
    (assert-true (error? (string/char-at "hi" 1000)))
    ```

13. **`string_index_finds_char`** — Tests: string/index finds character
    - **Category B** → Elle equivalent:
    ```lisp
    (assert-eq (string/index "hello" "l") 2)
    ```

14. **`string_index_not_found_returns_nil`** — Tests: string/index returns nil when not found
    - **Category B** → Elle equivalent:
    ```lisp
    (assert-eq (string/index "hello" "z") nil)
    ```

15. **`unicode_append_preserves_content`** — Tests: unicode append preserves content
    - **Category B** → Elle equivalent:
    ```lisp
    (assert-eq (append "hello" "world") "helloworld")
    ```

16. **`unicode_upcase_downcase_roundtrip`** — Tests: upcase/downcase roundtrip
    - **Category B** → Elle equivalent:
    ```lisp
    (assert-eq (string/downcase (string/upcase "hello")) "hello")
    ```

### fibers.rs (14 tests)
1. **`fiber_yield_resume_order`** — Tests: yields produce values in order
   - **Category B** → Elle equivalent:
   ```lisp
   (let ((f (fiber/new (fn () (fiber/signal 2 1) (fiber/signal 2 2) 3) 2)))
     (assert-eq (list (fiber/resume f) (fiber/resume f) (fiber/resume f))
                (list 1 2 3)))
   ```

2. **`signal_mask_catch_behavior`** — Tests: signal mask determines catch behavior
   - **Category B** → Elle equivalent:
   ```lisp
   (let ((f (fiber/new (fn () (fiber/signal 2 42) 99) 2)))
     (assert-eq (fiber/resume f) 42))
   ```

3. **`cancel_delivers_value_to_new_fiber`** — Tests: cancel delivers value
   - **Category B** → Elle equivalent:
   ```lisp
   (let ((f (fiber/new (fn () 42) 1)))
     (assert-eq (fiber/cancel f 99) 99))
   ```

4. **`cancel_delivers_value_to_suspended_fiber`** — Tests: cancel on suspended fiber
   - **Category B** → Elle equivalent:
   ```lisp
   (let ((f (fiber/new (fn () (fiber/signal 2 0) 99) 3)))
     (fiber/resume f)
     (assert-eq (fiber/cancel f 88) 88))
   ```

5. **`propagate_rejects_dead_fibers`** — Tests: propagate rejects dead fibers
   - **Category B** → Elle equivalent:
   ```lisp
   (let ((f (fiber/new (fn () 42) 0)))
     (fiber/resume f)
     (assert-true (error? (fiber/propagate f))))
   ```

6. **`propagate_succeeds_for_errored_fibers`** — Tests: propagate succeeds for errored
   - **Category B** → Elle equivalent:
   ```lisp
   (let ((f (fiber/new (fn () (fiber/signal 1 99) 42) 1)))
     (fiber/resume f)
     (assert-true (error? (fiber/propagate f))))
   ```

7. **`cancel_rejects_dead_fibers`** — Tests: cancel rejects dead fibers
   - **Category B** → Elle equivalent:
   ```lisp
   (let ((f (fiber/new (fn () 42) 0)))
     (fiber/resume f)
     (assert-true (error? (fiber/cancel f "too late"))))
   ```

8. **`cancel_accepts_suspended_after_caught_error`** — Tests: cancel accepts suspended
   - **Category B** → Elle equivalent:
   ```lisp
   (let ((f (fiber/new (fn () (fiber/signal 1 99) 42) 1)))
     (fiber/resume f)
     (fiber/cancel f "ok")
     (assert-eq (keyword->string (fiber/status f)) "error"))
   ```

9. **`cancel_rejects_errored_fibers`** — Tests: cancel rejects errored fibers
   - **Category B** → Elle equivalent:
   ```lisp
   (let ((f (fiber/new (fn () (fiber/signal 1 99) 42) 0)))
     (let ((wrapper (fiber/new (fn () (fiber/resume f)) 1)))
       (fiber/resume wrapper)
       (assert-true (error? (fiber/cancel f "already errored")))))
   ```

10. **`nested_fiber_resume_preserves_values`** — Tests: nested resume preserves values
    - **Category B** → Elle equivalent:
    ```lisp
    (let ((inner (fiber/new (fn () (fiber/signal 2 10) 99) 2)))
      (let ((outer (fiber/new (fn () (+ (fiber/resume inner) 5)) 0)))
        (assert-eq (fiber/resume outer) 15)))
    ```

11. **`multi_frame_yield_chain`** — Tests: yield propagates through call chain
    - **Category B** → Elle equivalent:
    ```lisp
    (begin
      (def helper (fn (x) (yield (* x 2))))
      (def caller (fn (x) (+ (helper x) 1)))
      (var co (make-coroutine (fn () (caller 5))))
      (assert-eq (coro/resume co) 10)
      (assert-eq (coro/resume co 7) 8))
    ```

12. **`re_yield_at_different_depth`** — Tests: re-yield at different depth
    - **Category B** → Elle equivalent:
    ```lisp
    (begin
      (def helper (fn (x) (yield x)))
      (def gen (fn () (helper 10) (yield 20) 42))
      (var co (make-coroutine gen))
      (assert-eq (coro/resume co) 10)
      (assert-eq (coro/resume co 5) 20)
      (assert-eq (coro/resume co) 42))
    ```

13. **`error_during_multi_frame_resume`** — Tests: error during multi-frame resume
    - **Category B** → Elle equivalent:
    ```lisp
    (begin
      (def helper (fn (x) (yield x) (/ 1 0)))
      (def gen (fn () (+ (helper 5) 1)))
      (var co (make-coroutine gen))
      (coro/resume co)
      (assert-true (error? (coro/resume co))))
    ```

14. **`three_level_nested_fiber_resume`** — Tests: 3-level nested resume
    - **Category B** → Elle equivalent:
    ```lisp
    (let ((c (fiber/new (fn () (fiber/signal 2 10) 99) 2)))
      (let ((b (fiber/new (fn () (+ (fiber/resume c) 5)) 0)))
        (let ((a (fiber/new (fn () (+ (fiber/resume b) 3)) 0)))
          (assert-eq (fiber/resume a) 18))))
    ```

### coroutines.rs (8 tests)
1. **`sequential_yields_in_order`** — Tests: sequential yields produce values in order
   - **Category B** → Elle equivalent:
   ```lisp
   (begin
     (def gen (fn () (yield 1) (yield 2) (yield 3) 4))
     (var co (make-coroutine gen))
     (assert-eq (list (coro/resume co) (coro/resume co) (coro/resume co) (coro/resume co))
                (list 1 2 3 4)))
   ```

2. **`resume_values_flow_into_yield`** — Tests: resume values flow into yield expressions
   - **Category B** → Elle equivalent:
   ```lisp
   (begin
     (def gen (fn () (let ((acc 0)) (set acc (+ acc (yield acc))) acc)))
     (var co (make-coroutine gen))
     (coro/resume co)
     (assert-eq (coro/resume co 10) 10))
   ```

3. **`yield_in_conditional`** — Tests: yield inside conditionals
   - **Category B** → Elle equivalent:
   ```lisp
   (begin
     (def gen (fn () (if true (yield 1) (yield 2))))
     (var co (make-coroutine gen))
     (assert-eq (coro/resume co) 1))
   ```

4. **`yield_in_loop`** — Tests: yield inside loops
   - **Category B** → Elle equivalent:
   ```lisp
   (begin
     (def gen (fn () (let ((i 0)) (while (< i 3) (begin (yield i) (set i (+ i 1)))) i)))
     (var co (make-coroutine gen))
     (assert-eq (list (coro/resume co) (coro/resume co) (coro/resume co) (coro/resume co))
                (list 0 1 2 3)))
   ```

5. **`coroutine_state_transitions`** — Tests: coroutine state machine
   - **Category B** → Elle equivalent:
   ```lisp
   (begin
     (def gen (fn () (yield 1) (yield 2) 3))
     (var co (make-coroutine gen))
     (assert-eq (keyword->string (coro/status co)) "created")
     (coro/resume co)
     (assert-eq (keyword->string (coro/status co)) "suspended")
     (coro/resume co)
     (assert-eq (keyword->string (coro/status co)) "suspended")
     (coro/resume co)
     (assert-eq (keyword->string (coro/status co)) "done"))
   ```

6. **`interleaved_coroutines`** — Tests: multiple interleaved coroutines
   - **Category B** → Elle equivalent:
   ```lisp
   (begin
     (def make-gen (fn (start) (fn () (yield (+ start 0)) (yield (+ start 1)) (+ start 2))))
     (var co1 (make-coroutine (make-gen 0)))
     (var co2 (make-coroutine (make-gen 100)))
     (assert-eq (list (coro/resume co1) (coro/resume co2) (coro/resume co1) (coro/resume co2))
                (list 0 100 1 101)))
   ```

7. **`yield_across_call_boundaries`** — Tests: yield across call boundaries
   - Fixed test (not proptest), tests yield in helper
   - **Category B** → Already fixed test, keep as-is

8. **`yield_across_two_call_levels`** — Tests: yield across two call levels
   - Fixed test (not proptest), tests yield through two calls
   - **Category B** → Already fixed test, keep as-is

---

## SUMMARY TABLE

| File | Category A | Category B | Total |
|------|-----------|-----------|-------|
| arithmetic.rs | 2 | 0 | 2 |
| comparison.rs | 4 | 0 | 4 |
| matching.rs | 0 | 4 | 4 |
| effects.rs | 13 | 0 | 13 |
| strings.rs | 0 | 16 | 16 |
| path.rs | 13 | 0 | 13 |
| reader.rs | 17 | 0 | 17 |
| nanboxing.rs | 26 | 0 | 26 |
| fibers.rs | 0 | 14 | 14 |
| coroutines.rs | 0 | 8 | 8 |
| ffi.rs | 45 | 0 | 45 |
| **TOTAL** | **120** | **42** | **162** |

---

## RECOMMENDATIONS

### 1. Keep as proptest (Category A)
All 120 tests in Category A should remain as proptest. They verify algebraic invariants that genuinely benefit from random input generation:
- Roundtrip fidelity (reader, NaN-boxing, FFI)
- Mathematical laws (commutativity, associativity, transitivity, idempotence)
- Type discrimination and invariants
- Range validation
- Structural properties

### 2. Migrate to Elle (Category B)
The 42 Category B tests should be migrated to `tests/elle/` as Elle test scripts:
- **matching.lisp** — 4 tests on match expressions
- **strings.lisp** — 16 tests on string operations
- **fibers.lisp** — 14 tests on fiber primitives
- **coroutines.lisp** — 8 tests on coroutine behavior

These tests don't need random generation — they're testing behavior with fixed representative inputs. Elle test scripts are more readable and maintainable.

### 3. Integration tests
The behavioral tests in `tests/integration/comparison.rs` (lines 1-87) could also move to Elle, but the float precision tests in `tests/integration/core.rs` must stay in Rust because Elle has no way to check float precision with tolerance.

---

## Migration Path

1. Create `tests/elle/matching.lisp` with 4 fixed tests
2. Create `tests/elle/strings.lisp` with 16 fixed tests
3. Create `tests/elle/fibers.lisp` with 14 fixed tests
4. Create `tests/elle/coroutines.lisp` with 8 fixed tests
5. Remove corresponding proptest blocks from property test files
6. Update `tests/property/mod.rs` to remove the migrated test modules
7. Update `tests/elle/mod.rs` to register the new test files

This will:
- Reduce property test file size by ~1,200 lines
- Make behavioral tests more readable (Elle syntax vs Rust format strings)
- Improve test maintainability (fixed inputs are easier to understand)
- Keep algebraic invariant tests where they belong (proptest)
