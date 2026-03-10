## Bytes and @bytes Type Tests
##
## Tests for the immutable bytes and mutable @bytes types.
## Migrated from tests/integration/bytes.rs

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ============================================================================
# Bytes and blob constructors
# ============================================================================

(assert-true (bytes? (bytes 72 101 108 108 111)) "bytes constructor creates bytes")
(assert-true (bytes? (bytes)) "empty bytes constructor")

(assert-true (bytes? (@bytes 72 101 108 108 111)) "@bytes constructor creates @bytes")
(assert-true (bytes? (@bytes)) "empty @bytes constructor")

# ============================================================================
# Bytes and blob predicates
# ============================================================================

(assert-true (bytes? (bytes 1 2 3)) "bytes? true for bytes")
(assert-true (bytes? (@bytes 1 2 3)) "bytes? true for @bytes")

# ============================================================================
# String to bytes/blob conversions
# ============================================================================

(assert-true (bytes? (string->bytes "hello")) "string->bytes returns bytes")
(assert-true (bytes? (string->blob "hello")) "string->blob returns @bytes")

# ============================================================================
# Bytes/blob to string conversions
# ============================================================================

(assert-eq (bytes->string (bytes 104 105)) "hi" "bytes->string")
(assert-eq (blob->string (@bytes 104 105)) "hi" "@bytes->string")

# ============================================================================
# Bytes/blob to hex conversions
# ============================================================================

(assert-eq (bytes->hex (bytes 72 101 108)) "48656c" "bytes->hex")
(assert-eq (blob->hex (@bytes 72 101 108)) "48656c" "@bytes->hex")

# ============================================================================
# Bytes and blob length
# ============================================================================

(assert-eq (length (bytes 1 2 3 4 5)) 5 "bytes length")
(assert-eq (length (@bytes 1 2 3 4 5)) 5 "@bytes length")

# ============================================================================
# Bytes and blob get
# ============================================================================

(assert-eq (get (bytes 72 101 108) 1) 101 "bytes get")
(assert-err (fn () (get (bytes 72 101 108) 10)) "bytes get out of bounds errors")

(assert-eq (get (@bytes 72 101 108) 1) 101 "@bytes get")
(assert-err (fn () (get (@bytes 72 101 108) 10)) "@bytes get out of bounds errors")

# ============================================================================
# URI encoding
# ============================================================================

(assert-eq (uri-encode "hello") "hello" "uri-encode simple")
(assert-eq (uri-encode "hello world") "hello%20world" "uri-encode space")
(assert-eq (uri-encode "a/b") "a%2Fb" "uri-encode special")

# ============================================================================
# @bytes mutations
# ============================================================================

(assert-true (bytes? (let ((b (@bytes 1 2))) (push b 3) b)) "@bytes push returns @bytes")
(assert-eq (let ((b (@bytes 1 2 3))) (pop b)) 3 "@bytes pop returns byte")
(assert-eq (let ((b (@bytes 1 2 3))) (put b 1 99) (get b 1)) 99 "@bytes put and get")

# ============================================================================
# Each over bytes and blob
# ============================================================================

(assert-eq (let ((sum 0))
             (each b (bytes 1 2 3)
               (assign sum (+ sum b)))
             sum)
           6
           "each over bytes")

(assert-eq (let ((sum 0))
             (each b (@bytes 10 20 30)
               (assign sum (+ sum b)))
             sum)
           60
           "each over @bytes")

# ============================================================================
# Map over bytes
# ============================================================================

(assert-eq (length (map (fn (b) (* b 2)) (bytes 1 2 3))) 3 "map over bytes returns list")

# ============================================================================
# @string to bytes/@bytes conversions
# ============================================================================

(assert-true (bytes? (buffer->bytes @"hello")) "buffer->bytes returns bytes")
(assert-true (bytes? (buffer->blob @"hello")) "buffer->blob returns @bytes")

# ============================================================================
# Bytes/@bytes to @string conversions
# ============================================================================

(assert-eq (freeze (bytes->buffer (bytes 104 105))) "hi" "bytes->buffer->string")
(assert-eq (freeze (blob->buffer (@bytes 104 105))) "hi" "@bytes->buffer->string")
