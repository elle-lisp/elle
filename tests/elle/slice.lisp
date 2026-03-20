## Slice Primitive Tests
##
## Tests for the generic `slice` primitive across all sequence types.


# ============================================================================
# Array slicing
# ============================================================================

(assert (= (slice [1 2 3 4 5] 1 3) [2 3]) "array slice middle")
(assert (= (slice [1 2 3 4 5] 0 5) [1 2 3 4 5]) "array slice full")
(assert (= (slice [1 2 3 4 5] 0 0) []) "array slice empty start=end=0")
(assert (= (slice [1 2 3 4 5] 3 3) []) "array slice empty start=end")
(assert (= (slice [1 2 3 4 5] 4 2) []) "array slice start > end")
(assert (= (slice [1 2 3 4 5] 0 3) [1 2 3]) "array slice from start")
(assert (= (slice [1 2 3 4 5] 3 5) [4 5]) "array slice to end")
(assert (= (slice [1 2 3] 0 100) [1 2 3]) "array slice end clamped")
(assert (= (slice [1 2 3] 100 200) []) "array slice start clamped past end")
(assert (array? (slice [1 2 3] 0 2)) "array slice returns array")

# ============================================================================
# Array slicing
# ============================================================================

(assert (= (slice @[1 2 3 4 5] 1 3) @[2 3]) "array slice middle")
(assert (= (slice @[1 2 3 4 5] 0 5) @[1 2 3 4 5]) "array slice full")
(assert (= (slice @[1 2 3 4 5] 0 0) @[]) "array slice empty start=end=0")
(assert (= (slice @[1 2 3 4 5] 3 3) @[]) "array slice empty start=end")
(assert (= (slice @[1 2 3 4 5] 4 2) @[]) "array slice start > end")
(assert (= (slice @[1 2 3] 0 100) @[1 2 3]) "array slice end clamped")
(assert (array? (slice @[1 2 3] 0 2)) "array slice returns array")

# ============================================================================
# List slicing
# ============================================================================

(assert (= (slice (list 1 2 3 4 5) 1 3) (list 2 3)) "list slice middle")
(assert (= (slice (list 1 2 3 4 5) 0 5) (list 1 2 3 4 5)) "list slice full")
(assert (= (slice (list 1 2 3 4 5) 0 0) (list)) "list slice empty start=end=0")
(assert (= (slice (list 1 2 3 4 5) 3 3) (list)) "list slice empty start=end")
(assert (= (slice (list 1 2 3 4 5) 4 2) (list)) "list slice start > end")
(assert (= (slice (list 1 2 3) 0 100) (list 1 2 3)) "list slice end clamped")
(assert (list? (slice (list 1 2 3) 0 2)) "list slice returns list")

# ============================================================================
# String slicing (grapheme-aware)
# ============================================================================

(assert (= (slice "hello" 1 4) "ell") "string slice middle")
(assert (= (slice "hello" 0 5) "hello") "string slice full")
(assert (= (slice "hello" 0 0) "") "string slice empty start=end=0")
(assert (= (slice "hello" 3 3) "") "string slice empty start=end")
(assert (= (slice "hello" 4 2) "") "string slice start > end")
(assert (= (slice "hello" 0 3) "hel") "string slice from start")
(assert (= (slice "hello" 3 5) "lo") "string slice to end")
(assert (= (slice "abc" 0 100) "abc") "string slice end clamped")
(assert (string? (slice "hello" 0 3)) "string slice returns string")

# ============================================================================
# @string slicing (grapheme-aware)
# Note: @string equality via = is reference-based, so we compare via freeze
# ============================================================================

(assert (= (freeze (slice @"hello" 1 4)) "ell") "@string slice middle")
(assert (= (freeze (slice @"hello" 0 5)) "hello") "@string slice full")
(assert (= (freeze (slice @"hello" 0 0)) "") "@string slice empty start=end=0")
(assert (= (freeze (slice @"hello" 3 3)) "") "@string slice empty start=end")
(assert (= (freeze (slice @"hello" 4 2)) "") "@string slice start > end")
(assert (string? (slice @"hello" 0 3)) "@string slice returns @string")

# ============================================================================
# Bytes slicing (existing behavior preserved)
# ============================================================================

(assert (= (slice (bytes 1 2 3 4 5) 1 3) (bytes 2 3)) "bytes slice middle")
(assert (= (slice (bytes 1 2 3) 0 100) (bytes 1 2 3)) "bytes slice end clamped")
(assert (bytes? (slice (bytes 1 2 3) 0 2)) "bytes slice returns bytes")

# ============================================================================
# @bytes slicing (existing behavior preserved)
# ============================================================================

(assert (= (slice (@bytes 1 2 3 4 5) 1 3) (@bytes 2 3)) "@bytes slice middle")
(assert (= (slice (@bytes 1 2 3) 0 100) (@bytes 1 2 3)) "@bytes slice end clamped")
(assert (bytes? (slice (@bytes 1 2 3) 0 2)) "@bytes slice returns @bytes")

# ============================================================================
# Error cases
# ============================================================================

(let (([ok? _] (protect ((fn () (slice 42 0 1)))))) (assert (not ok?) "slice on non-sequence errors"))
(let (([ok? _] (protect ((fn () (slice [1 2 3] -1 2)))))) (assert (not ok?) "slice negative start errors"))
(let (([ok? _] (protect ((fn () (slice [1 2 3] 0 -1)))))) (assert (not ok?) "slice negative end errors"))
