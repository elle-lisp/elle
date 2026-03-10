## Slice Primitive Tests
##
## Tests for the generic `slice` primitive across all sequence types.

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ============================================================================
# Tuple slicing
# ============================================================================

(assert-eq (slice [1 2 3 4 5] 1 3) [2 3] "tuple slice middle")
(assert-eq (slice [1 2 3 4 5] 0 5) [1 2 3 4 5] "tuple slice full")
(assert-eq (slice [1 2 3 4 5] 0 0) [] "tuple slice empty start=end=0")
(assert-eq (slice [1 2 3 4 5] 3 3) [] "tuple slice empty start=end")
(assert-eq (slice [1 2 3 4 5] 4 2) [] "tuple slice start > end")
(assert-eq (slice [1 2 3 4 5] 0 3) [1 2 3] "tuple slice from start")
(assert-eq (slice [1 2 3 4 5] 3 5) [4 5] "tuple slice to end")
(assert-eq (slice [1 2 3] 0 100) [1 2 3] "tuple slice end clamped")
(assert-eq (slice [1 2 3] 100 200) [] "tuple slice start clamped past end")
(assert-true (array? (slice [1 2 3] 0 2)) "tuple slice returns tuple")

# ============================================================================
# Array slicing
# ============================================================================

(assert-eq (slice @[1 2 3 4 5] 1 3) @[2 3] "array slice middle")
(assert-eq (slice @[1 2 3 4 5] 0 5) @[1 2 3 4 5] "array slice full")
(assert-eq (slice @[1 2 3 4 5] 0 0) @[] "array slice empty start=end=0")
(assert-eq (slice @[1 2 3 4 5] 3 3) @[] "array slice empty start=end")
(assert-eq (slice @[1 2 3 4 5] 4 2) @[] "array slice start > end")
(assert-eq (slice @[1 2 3] 0 100) @[1 2 3] "array slice end clamped")
(assert-true (array? (slice @[1 2 3] 0 2)) "array slice returns array")

# ============================================================================
# List slicing
# ============================================================================

(assert-eq (slice (list 1 2 3 4 5) 1 3) (list 2 3) "list slice middle")
(assert-eq (slice (list 1 2 3 4 5) 0 5) (list 1 2 3 4 5) "list slice full")
(assert-eq (slice (list 1 2 3 4 5) 0 0) (list) "list slice empty start=end=0")
(assert-eq (slice (list 1 2 3 4 5) 3 3) (list) "list slice empty start=end")
(assert-eq (slice (list 1 2 3 4 5) 4 2) (list) "list slice start > end")
(assert-eq (slice (list 1 2 3) 0 100) (list 1 2 3) "list slice end clamped")
(assert-true (list? (slice (list 1 2 3) 0 2)) "list slice returns list")

# ============================================================================
# String slicing (grapheme-aware)
# ============================================================================

(assert-eq (slice "hello" 1 4) "ell" "string slice middle")
(assert-eq (slice "hello" 0 5) "hello" "string slice full")
(assert-eq (slice "hello" 0 0) "" "string slice empty start=end=0")
(assert-eq (slice "hello" 3 3) "" "string slice empty start=end")
(assert-eq (slice "hello" 4 2) "" "string slice start > end")
(assert-eq (slice "hello" 0 3) "hel" "string slice from start")
(assert-eq (slice "hello" 3 5) "lo" "string slice to end")
(assert-eq (slice "abc" 0 100) "abc" "string slice end clamped")
(assert-true (string? (slice "hello" 0 3)) "string slice returns string")

# ============================================================================
# Buffer slicing (grapheme-aware)
# Note: buffer equality via = is reference-based, so we compare via buffer->string
# ============================================================================

(assert-eq (buffer->string (slice @"hello" 1 4)) "ell" "buffer slice middle")
(assert-eq (buffer->string (slice @"hello" 0 5)) "hello" "buffer slice full")
(assert-eq (buffer->string (slice @"hello" 0 0)) "" "buffer slice empty start=end=0")
(assert-eq (buffer->string (slice @"hello" 3 3)) "" "buffer slice empty start=end")
(assert-eq (buffer->string (slice @"hello" 4 2)) "" "buffer slice start > end")
(assert-true (string? (slice @"hello" 0 3)) "buffer slice returns buffer")

# ============================================================================
# Bytes slicing (existing behavior preserved)
# ============================================================================

(assert-eq (slice (bytes 1 2 3 4 5) 1 3) (bytes 2 3) "bytes slice middle")
(assert-eq (slice (bytes 1 2 3) 0 100) (bytes 1 2 3) "bytes slice end clamped")
(assert-true (bytes? (slice (bytes 1 2 3) 0 2)) "bytes slice returns bytes")

# ============================================================================
# Blob slicing (existing behavior preserved)
# ============================================================================

(assert-eq (slice (@bytes 1 2 3 4 5) 1 3) (@bytes 2 3) "blob slice middle")
(assert-eq (slice (@bytes 1 2 3) 0 100) (@bytes 1 2 3) "blob slice end clamped")
(assert-true (bytes? (slice (@bytes 1 2 3) 0 2)) "blob slice returns blob")

# ============================================================================
# Error cases
# ============================================================================

(assert-err (fn () (slice 42 0 1)) "slice on non-sequence errors")
(assert-err (fn () (slice [1 2 3] -1 2)) "slice negative start errors")
(assert-err (fn () (slice [1 2 3] 0 -1)) "slice negative end errors")
