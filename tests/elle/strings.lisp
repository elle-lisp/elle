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
# String comparison laws (migrated from tests/property/comparison.rs)
# ============================================================================

# < and >= are complementary
(assert-true (not (= (< "abc" "def") (>= "abc" "def")))
  "lt/ge complementary: abc vs def")
(assert-true (not (= (< "zzz" "aaa") (>= "zzz" "aaa")))
  "lt/ge complementary: zzz vs aaa")
(assert-true (not (= (< "same" "same") (>= "same" "same")))
  "lt/ge complementary: same vs same")

# > and <= are complementary
(assert-true (not (= (> "abc" "def") (<= "abc" "def")))
  "gt/le complementary: abc vs def")
(assert-true (not (= (> "zzz" "aaa") (<= "zzz" "aaa")))
  "gt/le complementary: zzz vs aaa")

# Transitivity: if a < b and b < c then a < c
(assert-true (if (and (< "abc" "def") (< "def" "ghi"))
               (< "abc" "ghi")
               true)
  "lt transitive: abc < def < ghi")

# <= is equivalent to (or (< a b) (= a b))
(assert-eq (<= "abc" "def") (or (< "abc" "def") (= "abc" "def"))
  "le = lt or eq: abc vs def")
(assert-eq (<= "abc" "abc") (or (< "abc" "abc") (= "abc" "abc"))
  "le = lt or eq: abc vs abc")
(assert-eq (<= "def" "abc") (or (< "def" "abc") (= "def" "abc"))
  "le = lt or eq: def vs abc")
