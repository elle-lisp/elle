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

(assert-true (bytes? (bytes "hello")) "bytes from string returns bytes")
(assert-true (bytes? (@bytes "hello")) "@bytes from string returns @bytes")

# ============================================================================
# Bytes/blob to string conversions
# ============================================================================

(assert-eq (string (bytes 104 105)) "hi" "bytes->string")
(assert-eq (freeze (string (@bytes 104 105))) "hi" "@bytes->string")

# ============================================================================
# Bytes/blob to hex conversions
# ============================================================================

(assert-eq (bytes->hex (bytes 72 101 108)) "48656c" "bytes->hex")
(assert-eq (freeze (bytes->hex (@bytes 72 101 108))) "48656c" "@bytes->hex")

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

(assert-true (bytes? (bytes @"hello")) "bytes from @string returns bytes")
(assert-true (bytes? (@bytes @"hello")) "@bytes from @string returns @bytes")

# ============================================================================
# Bytes/@bytes to @string conversions
# ============================================================================

(assert-eq (string (bytes 104 105)) "hi" "bytes->string via (string)")
(assert-eq (freeze (string (@bytes 104 105))) "hi" "@bytes->string via (string)")

# ============================================================================
# Mutability-preserving conversions
# ============================================================================

# (string x) preserves mutability: bytes→string, @bytes→@string
(assert-eq (type (string (bytes 104 105))) :string "string from bytes is immutable")
(assert-eq (type (string (@bytes 104 105))) :@string "string from @bytes is mutable")

# (string x) preserves mutability: string→string, @string→@string
(assert-eq (type (string "hello")) :string "string from string is immutable")
(assert-eq (type (string @"hello")) :@string "string from @string is mutable")

# (bytes x) preserves mutability: string→bytes, @string→@bytes
(assert-eq (type (bytes "hello")) :bytes "bytes from string is immutable")
(assert-eq (type (bytes @"hello")) :@bytes "bytes from @string is mutable")

# (bytes->hex x) preserves mutability: bytes→string, @bytes→@string
(assert-eq (type (bytes->hex (bytes 72 101 108))) :string "bytes->hex from bytes is immutable")
(assert-eq (type (bytes->hex (@bytes 72 101 108))) :@string "bytes->hex from @bytes is mutable")

# ============================================================================
# seq->hex — bytes input (same as bytes->hex, backward compat)
# ============================================================================

(assert-eq (seq->hex (bytes 72 101 108)) "48656c" "seq->hex bytes")
(assert-eq (freeze (seq->hex (@bytes 72 101 108))) "48656c" "seq->hex @bytes value")
(assert-eq (type (seq->hex (@bytes 72 101 108))) :@string "seq->hex @bytes is mutable")

# ============================================================================
# seq->hex — integer input (big-endian minimal bytes)
# ============================================================================

(assert-eq (seq->hex 0) "00" "seq->hex zero")
(assert-eq (seq->hex 255) "ff" "seq->hex 255")
(assert-eq (seq->hex 256) "0100" "seq->hex 256")
(assert-eq (seq->hex 65535) "ffff" "seq->hex 65535")
(assert-err-kind (fn () (seq->hex -1)) :value-error "seq->hex negative int errors")

# ============================================================================
# seq->hex — array input
# ============================================================================

(assert-eq (seq->hex [72 101 108]) "48656c" "seq->hex array")
(assert-eq (type (seq->hex [72 101 108])) :string "seq->hex array is immutable")
(assert-eq (freeze (seq->hex @[72 101 108])) "48656c" "seq->hex @array value")
(assert-eq (type (seq->hex @[72 101 108])) :@string "seq->hex @array is mutable")
(assert-err-kind (fn () (seq->hex [256])) :value-error "seq->hex array element out of range")
(assert-err-kind (fn () (seq->hex ["x"])) :type-error "seq->hex array element not int")

# ============================================================================
# seq->hex — list input
# ============================================================================

(assert-eq (seq->hex '(72 101 108)) "48656c" "seq->hex list")
(assert-eq (type (seq->hex '(72 101 108))) :string "seq->hex list is immutable")
(assert-err-kind (fn () (seq->hex '(256))) :value-error "seq->hex list element out of range")
(assert-err-kind (fn () (seq->hex '("x"))) :type-error "seq->hex list element not int")

# ============================================================================
# seq->hex — bytes->hex still works as alias
# ============================================================================

(assert-eq (bytes->hex (bytes 72 101 108)) "48656c" "bytes->hex alias still works")
