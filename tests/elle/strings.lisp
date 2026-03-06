## String Operation Law Tests
##
## Migrated from tests/property/strings.rs (input-invariant laws only).
## These are string operation laws that hold trivially once implemented —
## testing with random strings doesn't exercise different code paths.
##
## Boundary tests, unicode, and roundtrip tests remain as property tests
## in tests/property/strings.rs.

(import-file "./examples/assertions.lisp")

# ============================================================================
# Slice properties
# ============================================================================

# slice_full_range_is_identity: slicing the full range returns the original
(assert-string-eq (string/slice "hello" 0 5) "hello"
  "slice full range (hello)")
(assert-string-eq (string/slice "abc" 0 3) "abc"
  "slice full range (abc)")
(assert-string-eq (string/slice "" 0 0) ""
  "slice full range (empty)")

# slice_at_zero_to_zero: slicing from 0 to 0 returns empty string
(assert-string-eq (string/slice "hello" 0 0) ""
  "slice 0 to 0 (hello)")
(assert-string-eq (string/slice "abc" 0 0) ""
  "slice 0 to 0 (abc)")

# ============================================================================
# Append properties
# ============================================================================

# append_preserves_content: appending strings preserves both contents
(assert-string-eq (append "hello" " world") "hello world"
  "append hello and world")
(assert-string-eq (append "abc" "def") "abcdef"
  "append abc and def")

# append_empty_is_identity: appending empty string is identity
(assert-string-eq (append "hello" "") "hello"
  "append hello and empty")
(assert-string-eq (append "" "hello") "hello"
  "append empty and hello")
(assert-string-eq (append "" "") ""
  "append empty and empty")

# append_associative: append is associative
(assert-true (= (append (append "a" "b") "c") (append "a" (append "b" "c")))
  "append is associative")

# ============================================================================
# Case conversion
# ============================================================================

# upcase_downcase_roundtrip: upcase then downcase returns original
(assert-string-eq (string/downcase (string/upcase "hello")) "hello"
  "upcase then downcase (hello)")
(assert-string-eq (string/downcase (string/upcase "abc")) "abc"
  "upcase then downcase (abc)")

# upcase_idempotent: upcase is idempotent
(assert-true (= (string/upcase (string/upcase "ABC")) (string/upcase "ABC"))
  "upcase is idempotent")

# downcase_idempotent: downcase is idempotent
(assert-true (= (string/downcase (string/downcase "abc")) (string/downcase "abc"))
  "downcase is idempotent")

# upcase_preserves_content_length: upcase preserves length
(assert-true (= (length (string/upcase "hello")) (length "hello"))
  "upcase preserves length")

# downcase_preserves_content_length: downcase preserves length
(assert-true (= (length (string/downcase "HELLO")) (length "HELLO"))
  "downcase preserves length")

# ============================================================================
# Contains / starts-with / ends-with
# ============================================================================

# string_contains_self: a string contains itself
(assert-true (string/contains? "hello" "hello")
  "string contains itself")

# string_contains_empty: a string contains empty string
(assert-true (string/contains? "hello" "")
  "string contains empty")
(assert-true (string/contains? "" "")
  "empty contains empty")

# starts_with_self: a string starts with itself
(assert-true (string/starts-with? "hello" "hello")
  "string starts with itself")

# starts_with_empty: a string starts with empty string
(assert-true (string/starts-with? "hello" "")
  "string starts with empty")

# ends_with_self: a string ends with itself
(assert-true (string/ends-with? "hello" "hello")
  "string ends with itself")

# ends_with_empty: a string ends with empty string
(assert-true (string/ends-with? "hello" "")
  "string ends with empty")

# ============================================================================
# Replace
# ============================================================================

# replace_with_self_is_identity: replacing with same value is identity
(assert-string-eq (string/replace "hello world" "o" "o") "hello world"
  "replace o with o is identity")

# replace_empty_old_errors: replacing empty string should error
(assert-err (fn [] (string/replace "hello" "" "x"))
  "replace with empty old string errors")

# ============================================================================
# Trim
# ============================================================================

# trim_idempotent: trim is idempotent
(assert-true (= (string/trim (string/trim "  hello  ")) (string/trim "  hello  "))
  "trim is idempotent")

# trim_of_trimmed_is_noop: trimming already-trimmed string is noop
(assert-string-eq (string/trim "hello") "hello"
  "trim of trimmed is noop")

# trim_removes_whitespace: trim removes leading and trailing whitespace
(assert-string-eq (string/trim "   hello   ") "hello"
  "trim removes whitespace")

# ============================================================================
# Edge cases
# ============================================================================

# empty_string_operations: empty string operations
(assert-string-eq (append "" "") ""
  "append empty and empty")
(assert-string-eq (string/upcase "") ""
  "upcase empty")
(assert-string-eq (string/downcase "") ""
  "downcase empty")
(assert-string-eq (string/trim "") ""
  "trim empty")
(assert-string-eq (string/slice "" 0 0) ""
  "slice empty")

# whitespace_only_trim: trimming whitespace-only string returns empty
(assert-string-eq (string/trim "   ") ""
  "trim whitespace-only")

# single_character_operations: single character operations
(assert-true (= (length "a") 1)
  "length of single char")
(assert-string-eq (string/char-at "a" 0) "a"
  "char-at single char")

# ============================================================================
# Slice boundary checking (migrated from property tests)
# ============================================================================

# slice_start_end_order: valid range succeeds
(assert-string-eq (string/slice "hello" 0 2) "he"
  "slice valid range: he")
(assert-string-eq (string/slice "abcdef" 1 4) "bcd"
  "slice valid range: bcd")
(assert-string-eq (string/slice "test" 0 4) "test"
  "slice valid range: full string")

# slice_oob_end_returns_nil: OOB end index returns nil
(assert-eq (string/slice "hello" 0 100) nil
  "slice OOB end returns nil (hello)")
(assert-eq (string/slice "abc" 0 50) nil
  "slice OOB end returns nil (abc)")
(assert-eq (string/slice "" 0 1) nil
  "slice OOB end returns nil (empty)")

# slice_oob_start_returns_nil: OOB start index returns nil
(assert-eq (string/slice "hello" 100 101) nil
  "slice OOB start returns nil (hello)")
(assert-eq (string/slice "abc" 50 51) nil
  "slice OOB start returns nil (abc)")

# slice_reversed_range_returns_nil: reversed range returns nil
(assert-eq (string/slice "hello" 3 1) nil
  "slice reversed range returns nil (hello)")
(assert-eq (string/slice "abcdef" 5 2) nil
  "slice reversed range returns nil (abcdef)")

# ============================================================================
# Split / Join roundtrip (migrated from property tests)
# ============================================================================

# split_join_roundtrip: split then join recovers original
(assert-string-eq (string/join (string/split "a,b,c" ",") ",") "a,b,c"
  "split/join roundtrip: comma")
(assert-string-eq (string/join (string/split "x;y;z" ";") ";") "x;y;z"
  "split/join roundtrip: semicolon")
(assert-string-eq (string/join (string/split "one|two|three" "|") "|") "one|two|three"
  "split/join roundtrip: pipe")

# split_produces_list: split produces a list
(assert-true (pair? (string/split "a,b" ","))
  "split produces a cons cell")
(assert-true (pair? (string/split "hello" "l"))
  "split produces a cons cell (hello)")

# ============================================================================
# Conversion roundtrips (migrated from property tests)
# ============================================================================

# number_to_string_roundtrip: number->string->integer roundtrip
(assert-eq (string->integer (number->string 42)) 42
  "number->string->integer roundtrip: 42")
(assert-eq (string->integer (number->string -100)) -100
  "number->string->integer roundtrip: -100")
(assert-eq (string->integer (number->string 0)) 0
  "number->string->integer roundtrip: 0")

# string_to_integer_roundtrip: string->integer roundtrip
(assert-eq (string->integer "42") 42
  "string->integer: 42")
(assert-eq (string->integer "-100") -100
  "string->integer: -100")
(assert-eq (string->integer "0") 0
  "string->integer: 0")

# string_to_integer_invalid_returns_error: non-numeric string errors
(assert-err (fn [] (string->integer "abc"))
  "string->integer errors on abc")
(assert-err (fn [] (string->integer "hello"))
  "string->integer errors on hello")
(assert-err (fn [] (string->integer "xyz"))
  "string->integer errors on xyz")

# ============================================================================
# Index/char-at operations (migrated from property tests)
# ============================================================================

# char_at_valid_index: char-at returns single character
(assert-string-eq (string/char-at "hello" 0) "h"
  "char-at index 0")
(assert-string-eq (string/char-at "hello" 4) "o"
  "char-at index 4")
(assert-string-eq (string/char-at "abc" 1) "b"
  "char-at index 1")

# char_at_out_of_bounds_errors: char-at OOB errors
(assert-err (fn [] (string/char-at "hi" 1000))
  "char-at OOB errors (hi)")
(assert-err (fn [] (string/char-at "abc" 100))
  "char-at OOB errors (abc)")

# string_index_finds_char: string/index finds character
(assert-eq (string/index "hello" "l") 2
  "string/index finds l in hello")
(assert-eq (string/index "abcdef" "d") 3
  "string/index finds d in abcdef")
(assert-eq (string/index "test" "t") 0
  "string/index finds t in test (first occurrence)")

# string_index_not_found_returns_nil: string/index returns nil when not found
(assert-eq (string/index "hello" "z") nil
  "string/index returns nil for z in hello")
(assert-eq (string/index "abc" "x") nil
  "string/index returns nil for x in abc")

# ============================================================================
# Unicode operations (migrated from property tests)
# ============================================================================

# unicode_append_preserves_content: unicode append preserves content
(assert-string-eq (append "hello" "world") "helloworld"
  "unicode append: hello + world")
(assert-string-eq (append "café" "latte") "cafélatte"
  "unicode append: café + latte")
(assert-string-eq (append "" "test") "test"
  "unicode append: empty + test")

# unicode_upcase_downcase_roundtrip: upcase/downcase roundtrip (ASCII)
(assert-string-eq (string/downcase (string/upcase "hello")) "hello"
  "upcase/downcase roundtrip: hello")
(assert-string-eq (string/downcase (string/upcase "abc")) "abc"
  "upcase/downcase roundtrip: abc")
(assert-string-eq (string/downcase (string/upcase "xyz")) "xyz"
  "upcase/downcase roundtrip: xyz")

# ============================================================================
# string/format — positional interpolation
# ============================================================================

(assert-string-eq (string/format "{} + {} = {}" 1 2 3) "1 + 2 = 3"
  "format positional basic")

(assert-string-eq (string/format "Hello, {}!" "Alice") "Hello, Alice!"
  "format positional with string")

(assert-string-eq (string/format "{}" 42) "42"
  "format positional single")

(assert-string-eq (string/format "no placeholders") "no placeholders"
  "format no placeholders")

(assert-string-eq (string/format "literal {{braces}}") "literal {braces}"
  "format escaped braces")

# ============================================================================
# string/format — named interpolation
# ============================================================================

(assert-string-eq (string/format "{name} is {age}" :name "Alice" :age 30)
  "Alice is 30"
  "format named basic")

(assert-string-eq (string/format "{greeting}, {name}!" :greeting "Hello" :name "Bob")
  "Hello, Bob!"
  "format named multiple")

# ============================================================================
# string/format — format specs
# ============================================================================

(assert-string-eq (string/format "{:.2f}" 3.14159) "3.14"
  "format float precision")

(assert-string-eq (string/format "{:>10}" "hello") "     hello"
  "format right align")

(assert-string-eq (string/format "{:<10}" "hello") "hello     "
  "format left align")

(assert-string-eq (string/format "{:^10}" "hello") "  hello   "
  "format center align")

(assert-string-eq (string/format "{:05d}" 42) "00042"
  "format zero padding")

(assert-string-eq (string/format "{:x}" 255) "ff"
  "format hex lowercase")

(assert-string-eq (string/format "{:X}" 255) "FF"
  "format hex uppercase")

(assert-string-eq (string/format "{:o}" 8) "10"
  "format octal")

(assert-string-eq (string/format "{:b}" 10) "1010"
  "format binary")

(assert-string-eq (string/format "{:e}" 1000.0) "1e3"
  "format scientific")

(assert-string-eq (string/format "{:*^10}" "hi") "****hi****"
  "format custom fill center")

(assert-string-eq (string/format "{:>10d}" 42) "        42"
  "format right align integer")

# Default alignment: numbers right-align, strings left-align
(assert-string-eq (string/format "{:10}" 42) "        42"
  "format default align integer (right)")
(assert-string-eq (string/format "{:10}" "hi") "hi        "
  "format default align string (left)")

# Named with format specs
(assert-string-eq (string/format "{val:.2f}" :val 3.14159) "3.14"
  "format named with spec")

# ============================================================================
# string/format — error cases
# ============================================================================

# Positional arg count mismatch
(assert-err (fn [] (string/format "{} {}" 1))
  "format positional too few args")

(assert-err (fn [] (string/format "{}" 1 2))
  "format positional too many args")

# Named arg missing
(assert-err (fn [] (string/format "{name}" :other "value"))
  "format named missing key")

# Named arg extra
(assert-err (fn [] (string/format "{name}" :name "Alice" :extra "Bob"))
  "format named extra key")

# Mixed positional and named
(assert-err (fn [] (string/format "{} {name}" 1 :name "Alice"))
  "format mixed positional and named")

# Odd keyword args
(assert-err (fn [] (string/format "{name}" :name))
  "format odd keyword args")

# Non-keyword in named position
(assert-err (fn [] (string/format "{name}" 42 "Alice"))
  "format non-keyword in named position")

# Template not a string
(assert-err (fn [] (string/format 42))
  "format template not string")

# Invalid format spec
(assert-err (fn [] (string/format "{:z}" 42))
  "format invalid spec")

# Type mismatch: string with integer spec
(assert-err (fn [] (string/format "{:d}" "hello"))
  "format type mismatch string as d")
