## String and @string Type Tests
##
## Tests for immutable string and mutable @string types.
## @string constructor and operations, string operations on @strings.
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

(assert-err (fn () (put @"hello" 10 "x")) "put out of bounds errors")
(assert-err (fn () (put @"hello" -1 "x")) "put negative index errors")
(assert-err (fn () (put @"" 0 "x")) "put on empty @string errors")

# ============================================================================
# @string grapheme-consistent indexing
# ============================================================================

# length counts grapheme clusters, not bytes
(assert-eq (length @"café") 4 "length @\"café\" is 4 graphemes, not 5 bytes")
(assert-eq (length @"🎉🎊") 2 "length of emoji @string is grapheme count")
(assert-eq (length @"naïve") 5 "length @\"naïve\" counts combining sequence as one grapheme")

# put replaces grapheme cluster at the given position
(let ((s @"café"))
  (put s 3 "E")
  (assert-eq (freeze s) "cafE" "put replaces grapheme at index 3"))

(let ((s @"hello"))
  (put s 0 "H")
  (assert-eq (freeze s) "Hello" "put replaces first grapheme"))

(let ((s @"hello"))
  (put s 4 "O")
  (assert-eq (freeze s) "hellO" "put replaces last grapheme"))

# put accepts multi-byte replacement string
(let ((s @"cafe"))
  (put s 3 "é")
  (assert-eq (freeze s) "café" "put can replace with multi-byte grapheme"))

# round-trip: get then put restores original
(let ((s @"café"))
  (let ((g (get s 3)))
    (put s 3 "E")
    (put s 3 g)
    (assert-eq (freeze s) "café" "get/put round-trip preserves original value")))

# put type errors
(assert-err (fn () (put @"hello" 0 88)) "put @string rejects integer value")
(assert-err (fn () (put @"hello" "a" "b")) "put @string rejects non-integer index")

# put bounds errors (use string values, matching new semantics)
(assert-err (fn () (put @"hello" 10 "x")) "put out of bounds errors (new)")
(assert-err (fn () (put @"hello" -1 "x")) "put negative index errors (new)")
(assert-err (fn () (put @"" 0 "x")) "put on empty @string errors (new)")

# ============================================================================
# @string pop
# ============================================================================

(assert-eq (begin (var b @"hi") (pop b)) "i" "pop returns last grapheme as string")
(assert-eq (begin (var b @"café") (pop b)) "é" "pop returns last multibyte grapheme")
(assert-err (fn () (begin (var b @"") (pop b))) "pop on empty @string errors")

# ============================================================================
# @string push
# ============================================================================

(assert-true (string? (begin (var b @"hi") (push b "!") b)) "push returns @string")
(assert-eq (freeze (begin (var b @"hi") (push b "!") b)) "hi!" "push appends string to @string")
(assert-eq (freeze (begin (var b @"café") (push b "x") b)) "caféx" "push appends to multibyte @string")
(assert-err (fn () (begin (var b @"hi") (push b 33))) "push rejects integer for @string")

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
