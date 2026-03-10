## Buffer Type Tests
##
## Tests for the mutable buffer type (@"..." literals and operations).
## Migrated from tests/integration/buffer.rs

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ============================================================================
# Buffer literals and constructors
# ============================================================================

(assert-true (string? @"hello") "buffer literal is buffer")
(assert-true (string? @"") "empty buffer literal is buffer")
(assert-true (string? (@string)) "buffer constructor creates buffer")
(assert-true (string? (@string 72 101 108 108 111)) "buffer with bytes is buffer")

# ============================================================================
# Buffer predicates
# ============================================================================

(assert-true (string? @"hello") "string? true for buffer")
(assert-true (string? "hello") "string? true for string")

# ============================================================================
# Buffer length
# ============================================================================

(assert-eq (length @"hello") 5 "length of @\"hello\"")
(assert-eq (length @"") 0 "length of empty buffer")

# ============================================================================
# Buffer empty predicate
# ============================================================================

(assert-true (empty? @"") "empty? true for empty buffer")
(assert-false (empty? @"hello") "empty? false for non-empty buffer")

# ============================================================================
# Buffer get
# ============================================================================

# Buffer get returns grapheme cluster as string, not byte
(assert-eq (get @"hello" 0) "h" "get @\"hello\" 0")
(assert-eq (get @"hello" 2) "l" "get @\"hello\" 2")
(assert-eq (get @"hello" 4) "o" "get @\"hello\" 4")
(assert-eq (get @"hello" 100) nil "get out of bounds returns nil")
(assert-eq (get @"hello" 100 99) 99 "get with default")

# ============================================================================
# Buffer put
# ============================================================================

(assert-err (fn () (put @"hello" 10 88)) "put out of bounds errors")
(assert-err (fn () (put @"hello" -1 88)) "put negative index errors")
(assert-err (fn () (put @"" 0 88)) "put on empty buffer errors")

# ============================================================================
# Buffer pop
# ============================================================================

(assert-eq (begin (var b @"hi") (pop b)) 105 "pop returns byte value (i=105)")
(assert-err (fn () (begin (var b @"") (pop b))) "pop on empty buffer errors")

# ============================================================================
# Buffer push
# ============================================================================

(assert-true (string? (begin (var b @"hi") (push b 33) b)) "push returns buffer")

# ============================================================================
# Buffer append
# ============================================================================

(assert-true (string? (begin (var b @"hello") (append b @" world") b)) "append returns buffer")

# ============================================================================
# Buffer concat
# ============================================================================

(assert-true (string? (concat @"hello" @" world")) "concat returns buffer")

# ============================================================================
# Buffer roundtrip conversions
# ============================================================================

(assert-eq (buffer->string (string->buffer "hello")) "hello" "string->buffer->string roundtrip")
(assert-eq (buffer->string @"hello") "hello" "buffer->string literal")

# ============================================================================
# Buffer insert
# ============================================================================

(assert-true (string? (begin (var b @"hllo") (insert b 1 101) b)) "insert returns buffer")

# ============================================================================
# Buffer remove
# ============================================================================

(assert-true (string? (begin (var b @"hello") (remove b 1) b)) "remove returns buffer")
(assert-true (string? (begin (var b @"hello") (remove b 1 2) b)) "remove multiple returns buffer")

# ============================================================================
# Buffer popn
# ============================================================================

(assert-true (string? (begin (var b @"hello") (popn b 2))) "popn returns buffer")

# ============================================================================
# String operations on buffers
# ============================================================================

(assert-true (string/contains? @"hello world" "world") "buffer contains substring")
(assert-false (string/contains? @"hello" "xyz") "buffer doesn't contain substring")

(assert-true (string/starts-with? @"hello" "he") "buffer starts with prefix")
(assert-false (string/starts-with? @"hello" "lo") "buffer doesn't start with suffix")

(assert-true (string/ends-with? @"hello" "lo") "buffer ends with suffix")
(assert-false (string/ends-with? @"hello" "he") "buffer doesn't end with prefix")

(assert-eq (string/index @"hello" "l") 2 "buffer index of substring")
(assert-eq (string/index @"hello" "z") nil "buffer index not found")

(assert-eq (buffer->string (slice @"hello" 1 4)) "ell" "slice of buffer")

(assert-true (string? (string/upcase @"hello")) "upcase buffer returns buffer")
(assert-true (string? (string/downcase @"HELLO")) "downcase buffer returns buffer")

(assert-true (string? (string/trim @"  hello  ")) "trim buffer returns buffer")

(assert-eq (get @"hello" 1) "e" "get on buffer returns e")

# ============================================================================
# Buffer split
# ============================================================================

(assert-eq (length (string/split @"a,b,c" ",")) 3 "split buffer returns 3 parts")

# ============================================================================
# Buffer replace
# ============================================================================

(assert-true (string? (string/replace @"hello" "l" "L")) "replace on buffer returns buffer")

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
