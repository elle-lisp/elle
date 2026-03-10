## @string Type Tests
##
## Tests for the mutable @string type (@"..." literals and operations).
## Migrated from tests/integration/buffer.rs

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ============================================================================
# @string literals and constructors
# ============================================================================

(assert-true (string? @"hello") "@string literal is @string")
(assert-true (string? @"") "empty @string literal is @string")
(assert-true (string? (@string)) "@string constructor creates @string")
(assert-true (string? (@string 72 101 108 108 111)) "@string with bytes is @string")

# ============================================================================
# @string predicates
# ============================================================================

(assert-true (string? @"hello") "string? true for @string")
(assert-true (string? "hello") "string? true for string")

# ============================================================================
# @string length
# ============================================================================

(assert-eq (length @"hello") 5 "length of @\"hello\"")
(assert-eq (length @"") 0 "length of empty buffer")

# ============================================================================
# @string empty predicate
# ============================================================================

(assert-true (empty? @"") "empty? true for empty @string")
(assert-false (empty? @"hello") "empty? false for non-empty @string")

# ============================================================================
# @string get
# ============================================================================

# @string get returns grapheme cluster as string, not byte
(assert-eq (get @"hello" 0) "h" "get @\"hello\" 0")
(assert-eq (get @"hello" 2) "l" "get @\"hello\" 2")
(assert-eq (get @"hello" 4) "o" "get @\"hello\" 4")
(assert-eq (get @"hello" 100) nil "get out of bounds returns nil")
(assert-eq (get @"hello" 100 99) 99 "get with default")

# ============================================================================
# @string put
# ============================================================================

(assert-err (fn () (put @"hello" 10 88)) "put out of bounds errors")
(assert-err (fn () (put @"hello" -1 88)) "put negative index errors")
(assert-err (fn () (put @"" 0 88)) "put on empty @string errors")

# ============================================================================
# @string pop
# ============================================================================

(assert-eq (begin (var b @"hi") (pop b)) 105 "pop returns byte value (i=105)")
(assert-err (fn () (begin (var b @"") (pop b))) "pop on empty @string errors")

# ============================================================================
# @string push
# ============================================================================

(assert-true (string? (begin (var b @"hi") (push b 33) b)) "push returns @string")

# ============================================================================
# @string append
# ============================================================================

(assert-true (string? (begin (var b @"hello") (append b @" world") b)) "append returns @string")

# ============================================================================
# @string concat
# ============================================================================

(assert-true (string? (concat @"hello" @" world")) "concat returns @string")

# ============================================================================
# @string roundtrip conversions
# ============================================================================

(assert-eq (freeze (thaw "hello")) "hello" "freeze/thaw string roundtrip")
(assert-eq (freeze @"hello") "hello" "freeze @string literal")

# ============================================================================
# @string insert
# ============================================================================

(assert-true (string? (begin (var b @"hllo") (insert b 1 101) b)) "insert returns @string")

# ============================================================================
# @string remove
# ============================================================================

(assert-true (string? (begin (var b @"hello") (remove b 1) b)) "remove returns @string")
(assert-true (string? (begin (var b @"hello") (remove b 1 2) b)) "remove multiple returns @string")

# ============================================================================
# @string popn
# ============================================================================

(assert-true (string? (begin (var b @"hello") (popn b 2))) "popn returns @string")

# ============================================================================
# String operations on @strings
# ============================================================================

(assert-true (string/contains? @"hello world" "world") "@string contains substring")
(assert-false (string/contains? @"hello" "xyz") "@string doesn't contain substring")

(assert-true (string/starts-with? @"hello" "he") "@string starts with prefix")
(assert-false (string/starts-with? @"hello" "lo") "@string doesn't start with suffix")

(assert-true (string/ends-with? @"hello" "lo") "@string ends with suffix")
(assert-false (string/ends-with? @"hello" "he") "@string doesn't end with prefix")

(assert-eq (string/index @"hello" "l") 2 "@string index of substring")
(assert-eq (string/index @"hello" "z") nil "@string index not found")

(assert-eq (freeze (slice @"hello" 1 4)) "ell" "slice of @string")

(assert-true (string? (string/upcase @"hello")) "upcase @string returns @string")
(assert-true (string? (string/downcase @"HELLO")) "downcase @string returns @string")

(assert-true (string? (string/trim @"  hello  ")) "trim @string returns @string")

(assert-eq (get @"hello" 1) "e" "get on @string returns e")

# ============================================================================
# @string split
# ============================================================================

(assert-eq (length (string/split @"a,b,c" ",")) 3 "split @string returns 3 parts")

# ============================================================================
# @string replace
# ============================================================================

(assert-true (string? (string/replace @"hello" "l" "L")) "replace on @string returns @string")

# ============================================================================
# Concat on lists
# ============================================================================

(assert-eq (length (concat (list 1 2) (list 3 4))) 4 "concat lists length")
(assert-eq (length (concat (list) (list 1 2))) 2 "concat with empty list")

# Verify original lists unchanged
(assert-eq (let ((a (list 1 2)))
             (let ((b (concat a (list 3 4))))
               (list (length a) (length b))))
           (list 2 4)
           "concat doesn't modify original lists")
