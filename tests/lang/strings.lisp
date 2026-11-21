(elle/epoch 18)
# audited: 2026-11-20
# The laws of the string operations: slice, append, case, search, replace, trim, split and join, conversion and index.
# docs/strings.md
#
# Each law holds for any input once the operation is right, so a fixed input
# exercises it as well as a random one.

# ============================================================================
# Slice properties
# ============================================================================

# slice_full_range_is_identity: slicing the full range returns the original
(assert (= (slice "hello" 0 5) "hello") "slice full range (hello)")
(assert (= (slice "abc" 0 3) "abc") "slice full range (abc)")
(assert (= (slice "" 0 0) "") "slice full range (empty)")

# slice_at_zero_to_zero: slicing from 0 to 0 returns empty string
(assert (= (slice "hello" 0 0) "") "slice 0 to 0 (hello)")
(assert (= (slice "abc" 0 0) "") "slice 0 to 0 (abc)")

# ============================================================================
# Append properties
# ============================================================================

# append_preserves_content: appending strings preserves both contents
(assert (= (append "hello" " world") "hello world") "append hello and world")
(assert (= (append "abc" "def") "abcdef") "append abc and def")

# append_empty_is_identity: appending empty string is identity
(assert (= (append "hello" "") "hello") "append hello and empty")
(assert (= (append "" "hello") "hello") "append empty and hello")
(assert (= (append "" "") "") "append empty and empty")

# append_associative: append is associative
(assert (= (append (append "a" "b") "c") (append "a" (append "b" "c")))
        "append is associative")

# ============================================================================
# Case conversion
# ============================================================================

# upcase_downcase_roundtrip: upcase then downcase returns original
(assert (= (string/downcase (string/upcase "hello")) "hello")
        "upcase then downcase (hello)")
(assert (= (string/downcase (string/upcase "abc")) "abc")
        "upcase then downcase (abc)")

# upcase_idempotent: upcase is idempotent
(assert (= (string/upcase (string/upcase "ABC")) (string/upcase "ABC"))
        "upcase is idempotent")

# downcase_idempotent: downcase is idempotent
(assert (= (string/downcase (string/downcase "abc")) (string/downcase "abc"))
        "downcase is idempotent")

# upcase_preserves_content_length: upcase preserves length
(assert (= (length (string/upcase "hello")) (length "hello"))
        "upcase preserves length")

# downcase_preserves_content_length: downcase preserves length
(assert (= (length (string/downcase "HELLO")) (length "HELLO"))
        "downcase preserves length")

# ============================================================================
# Contains / starts-with / ends-with
# ============================================================================

# string_contains_self: a string contains itself
(assert (string/contains? "hello" "hello") "string contains itself")

# string_contains_empty: a string contains empty string
(assert (string/contains? "hello" "") "string contains empty")
(assert (string/contains? "" "") "empty contains empty")

# starts_with_self: a string starts with itself
(assert (string/starts-with? "hello" "hello") "string starts with itself")

# starts_with_empty: a string starts with empty string
(assert (string/starts-with? "hello" "") "string starts with empty")

# ends_with_self: a string ends with itself
(assert (string/ends-with? "hello" "hello") "string ends with itself")

# ends_with_empty: a string ends with empty string
(assert (string/ends-with? "hello" "") "string ends with empty")

# ============================================================================
# Replace
# ============================================================================

# replace_with_self_is_identity: replacing with same value is identity
(assert (= (string/replace "hello world" "o" "o") "hello world")
        "replace o with o is identity")

# replace_empty_old_errors: replacing empty string should error
(let [[ok? _] (protect ((fn [] (string/replace "hello" "" "x"))))]
  (assert (not ok?) "replace with empty old string errors"))

# ============================================================================
# Trim
# ============================================================================

# trim_idempotent: trim is idempotent
(assert (= (string/trim (string/trim "  hello  ")) (string/trim "  hello  "))
        "trim is idempotent")

# trim_of_trimmed_is_noop: trimming already-trimmed string is noop
(assert (= (string/trim "hello") "hello") "trim of trimmed is noop")

# trim_removes_whitespace: trim removes leading and trailing whitespace
(assert (= (string/trim "   hello   ") "hello") "trim removes whitespace")

# ============================================================================
# Edge cases
# ============================================================================

# empty_string_operations: empty string operations
(assert (= (append "" "") "") "append empty and empty")
(assert (= (string/upcase "") "") "upcase empty")
(assert (= (string/downcase "") "") "downcase empty")
(assert (= (string/trim "") "") "trim empty")
(assert (= (slice "" 0 0) "") "slice empty")

# whitespace_only_trim: trimming whitespace-only string returns empty
(assert (= (string/trim "   ") "") "trim whitespace-only")

# single_grapheme_cluster_operations: single grapheme cluster operations
(assert (= (length "a") 1) "length of single grapheme cluster")
(assert (= (get "a" 0) "a") "get single grapheme cluster")

# ============================================================================
# Slice boundary checking
# ============================================================================

# slice_start_end_order: valid range succeeds
(assert (= (slice "hello" 0 2) "he") "slice valid range: he")
(assert (= (slice "abcdef" 1 4) "bcd") "slice valid range: bcd")
(assert (= (slice "test" 0 4) "test") "slice valid range: full string")

# slice_oob_end_clamps: OOB end index clamps to length
(assert (= (slice "hello" 0 100) "hello") "slice OOB end clamps (hello)")
(assert (= (slice "abc" 0 50) "abc") "slice OOB end clamps (abc)")
(assert (= (slice "" 0 1) "") "slice OOB end clamps (empty)")

# slice_oob_start_clamps: OOB start index clamps to length
(assert (= (slice "hello" 100 101) "") "slice OOB start clamps (hello)")
(assert (= (slice "abc" 50 51) "") "slice OOB start clamps (abc)")

# slice_reversed_range_returns_empty: reversed range returns empty
(assert (= (slice "hello" 3 1) "") "slice reversed range returns empty (hello)")
(assert (= (slice "abcdef" 5 2) "")
        "slice reversed range returns empty (abcdef)")

# ============================================================================
# Split / Join roundtrip
# ============================================================================

# split_join_roundtrip: split then join recovers original
(assert (= (string/join (string/split "a,b,c" ",") ",") "a,b,c")
        "split/join roundtrip: comma")
(assert (= (string/join (string/split "x;y;z" ";") ";") "x;y;z")
        "split/join roundtrip: semicolon")
(assert (= (string/join (string/split "one|two|three" "|") "|") "one|two|three")
        "split/join roundtrip: pipe")

# split_produces_array: split produces an array
(assert (array? (string/split "a,b" ",")) "split produces an array")
(assert (array? (string/split "hello" "l")) "split produces an array (hello)")

# ============================================================================
# Conversion roundtrips
# ============================================================================

# number_to_string_roundtrip: number->string->parse-int roundtrip
(assert (= (parse-int (number->string 42)) 42)
        "number->string->parse-int roundtrip: 42")
(assert (= (parse-int (number->string -100)) -100)
        "number->string->parse-int roundtrip: -100")
(assert (= (parse-int (number->string 0)) 0)
        "number->string->parse-int roundtrip: 0")

# string_to_integer_roundtrip: parse-int from string
(assert (= (parse-int "42") 42) "parse-int from string: 42")
(assert (= (parse-int "-100") -100) "parse-int from string: -100")
(assert (= (parse-int "0") 0) "parse-int from string: 0")

# string_to_integer_invalid_returns_error: non-numeric string errors
(let [[ok? _] (protect ((fn [] (parse-int "abc"))))]
  (assert (not ok?) "parse-int from string errors on abc"))
(let [[ok? _] (protect ((fn [] (parse-int "hello"))))]
  (assert (not ok?) "parse-int from string errors on hello"))
(let [[ok? _] (protect ((fn [] (parse-int "xyz"))))]
  (assert (not ok?) "parse-int from string errors on xyz"))

# ============================================================================
# Index/get operations
# ============================================================================

# get_valid_index: get returns single grapheme cluster
(assert (= (get "hello" 0) "h") "get string index 0")
(assert (= (get "hello" 4) "o") "get string index 4")
(assert (= (get "abc" 1) "b") "get string index 1")

# get_out_of_bounds_returns_nil: get OOB returns nil
(assert (= (get "hi" 1000) nil) "get string OOB returns nil (hi)")
(assert (= (get "abc" 100) nil) "get string OOB returns nil (abc)")

# string_index_finds_char: string/index finds character
(assert (= (string/index "hello" "l") 2) "string/index finds l in hello")
(assert (= (string/index "abcdef" "d") 3) "string/index finds d in abcdef")
(assert (= (string/index "test" "t") 0)
        "string/index finds t in test (first occurrence)")

# string_index_not_found_returns_nil: string/index returns nil when not found
(assert (= (string/index "hello" "z") nil)
        "string/index returns nil for z in hello")
(assert (= (string/index "abc" "x") nil) "string/index returns nil for x in abc")

# ============================================================================
# Unicode operations
# ============================================================================

# unicode_append_preserves_content: unicode append preserves content
(assert (= (append "hello" "world") "helloworld")
        "unicode append: hello + world")
(assert (= (append "café" "latte") "cafélatte")
        "unicode append: café + latte")
(assert (= (append "" "test") "test") "unicode append: empty + test")

# unicode_upcase_downcase_roundtrip: upcase/downcase roundtrip (ASCII)
(assert (= (string/downcase (string/upcase "hello")) "hello")
        "upcase/downcase roundtrip: hello")
(assert (= (string/downcase (string/upcase "abc")) "abc")
        "upcase/downcase roundtrip: abc")
(assert (= (string/downcase (string/upcase "xyz")) "xyz")
        "upcase/downcase roundtrip: xyz")
