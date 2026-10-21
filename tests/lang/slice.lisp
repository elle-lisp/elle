(elle/epoch 15)
# audited: 2026-10-20
## slice answers the piece between two indices of any sequence, and the piece is a value of its own.
## docs/bytes.md
## docs/strings.md
##
## The traps: a negative index counts from the end and clamps at zero; a
## start at or past its end is empty, never an error; `slice` on a string
## counts graphemes, not bytes; a slice of a mutable value is mutable.
##
## The counter-factual: a slice that copied passes every assertion here.
## What a slice shares is an implementation claim, pinned by
## tests/impl/bytes-view.lisp. This file pins the values every implementation
## answers.

## ── arrays ──────────────────────────────────────────────────────────────

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
(assert (= (slice [1 2 3 4] -1) [4]) "a negative index on an array")

(assert (= (slice @[1 2 3 4 5] 1 3) @[2 3]) "@array slice middle")
(assert (= (slice @[1 2 3 4 5] 0 5) @[1 2 3 4 5]) "@array slice full")
(assert (= (slice @[1 2 3 4 5] 0 0) @[]) "@array slice empty start=end=0")
(assert (= (slice @[1 2 3 4 5] 3 3) @[]) "@array slice empty start=end")
(assert (= (slice @[1 2 3 4 5] 4 2) @[]) "@array slice start > end")
(assert (= (slice @[1 2 3] 0 100) @[1 2 3]) "@array slice end clamped")
(assert (array? (slice @[1 2 3] 0 2)) "@array slice returns array")
(println "  1. arrays")

## ── lists ───────────────────────────────────────────────────────────────

(assert (= (slice (list 1 2 3 4 5) 1 3) (list 2 3)) "list slice middle")
(assert (= (slice (list 1 2 3 4 5) 0 5) (list 1 2 3 4 5)) "list slice full")
(assert (= (slice (list 1 2 3 4 5) 0 0) (list)) "list slice empty start=end=0")
(assert (= (slice (list 1 2 3 4 5) 3 3) (list)) "list slice empty start=end")
(assert (= (slice (list 1 2 3 4 5) 4 2) (list)) "list slice start > end")
(assert (= (slice (list 1 2 3) 0 100) (list 1 2 3)) "list slice end clamped")
(assert (list? (slice (list 1 2 3) 0 2)) "list slice returns list")
(println "  2. lists")

## ── strings, by grapheme ────────────────────────────────────────────────

(assert (= (slice "hello" 1 4) "ell") "string slice middle")
(assert (= (slice "hello" 0 5) "hello") "string slice full")
(assert (= (slice "hello" 0 0) "") "string slice empty start=end=0")
(assert (= (slice "hello" 3 3) "") "string slice empty start=end")
(assert (= (slice "hello" 4 2) "") "string slice start > end")
(assert (= (slice "hello" 0 3) "hel") "string slice from start")
(assert (= (slice "hello" 3 5) "lo") "string slice to end")
(assert (= (slice "abc" 0 100) "abc") "string slice end clamped")
(assert (= (slice "hello" 2) "llo") "the end defaults to the length")
(assert (= (slice "hello" -3) "llo") "a negative index counts from the end")
(assert (= (slice "hello" 5) "") "a start at the length is empty")
(assert (= (slice "héllo" 1 3) "él") "a multi-byte grapheme is one index")
(assert (= (slice "a👋🏽b" 1 2) "👋🏽")
        "a cluster of two codepoints is one index")
(assert (string? (slice "hello" 0 3)) "string slice returns string")
(assert (= (type-of (slice "hello" 1 4)) :string) "and immutable")
(assert (= (slice (slice "abcdef" 1) 1 3) "cd") "a slice of a slice")

## @string equality via = is reference-based, so the value is read through freeze.
(assert (= (freeze (slice @"hello" 1 4)) "ell") "@string slice middle")
(assert (= (freeze (slice @"hello" 0 5)) "hello") "@string slice full")
(assert (= (freeze (slice @"hello" 0 0)) "") "@string slice empty start=end=0")
(assert (= (freeze (slice @"hello" 3 3)) "") "@string slice empty start=end")
(assert (= (freeze (slice @"hello" 4 2)) "") "@string slice start > end")
(assert (string? (slice @"hello" 0 3)) "@string slice returns @string")
(assert (= (type-of (slice @"hello" 1 4)) :@string)
        "a slice of @string is @string")
(println "  3. strings")

## ── bytes ───────────────────────────────────────────────────────────────

(def b (bytes 1 2 3 4 5))

(assert (= (slice b 1 3) (bytes 2 3)) "the bytes between the two indices")
(assert (= (slice b 3) (bytes 4 5)) "the end defaults to the length")
(assert (= (slice b -2) (bytes 4 5)) "a negative index counts from the end")
(assert (= (slice b -100 2) (bytes 1 2)) "a negative index clamps at zero")
(assert (= (slice b 0 100) b) "an end past the length clamps to it")
(assert (= (slice b 4 2) (bytes)) "a start past its end is empty")
(assert (= (slice b 2 2) (bytes)) "a start at its end is empty")
(assert (bytes? (slice b 1 3)) "a slice of bytes is bytes")
(assert (= (type-of (slice b 1 3)) :bytes) "and immutable")
(assert (= (slice (slice b 1) 1 3) (bytes 3 4)) "a slice of a slice")
(assert (= (get (slice b 1) 0) 2) "get indexes from the slice's start")
(assert (= (length (slice b 1 4)) 3) "length is the slice's own")

(assert (= (slice (@bytes 1 2 3 4 5) 1 3) (@bytes 2 3)) "@bytes slice middle")
(assert (= (slice (@bytes 1 2 3) 0 100) (@bytes 1 2 3))
        "@bytes slice end clamped")
(assert (bytes? (slice (@bytes 1 2 3) 0 2)) "@bytes slice returns @bytes")
(assert (= (type-of (slice (@bytes 1 2 3) 1)) :@bytes)
        "a slice of @bytes is @bytes")
(println "  4. bytes")

## ── negative indices and errors ─────────────────────────────────────────

(let [[ok? _] (protect ((fn () (slice 42 0 1))))]
  (assert (not ok?) "slice on non-sequence errors"))
(assert (= (slice [1 2 3] -1 3) [3]) "slice negative start resolves")
(assert (= (slice [1 2 3] 0 -1) [1 2]) "slice negative end resolves")
(assert (= (slice [1 2 3] -2 -1) [2]) "slice both negative")
(assert (= (slice "hello" -3 -1) "ll") "slice string negative")
(println "  5. negative indices and errors")

## ── a slice is a value of its own ───────────────────────────────────────

(defn tail-of [n]
  "The source dies with this call's frame; the slice goes on."
  (let [s (string/repeat "ab" n)]
    (slice s 2)))

(assert (= (tail-of 3) "abab")
        "a slice outlives the frame its source was born in")

(let [@s (bytes 1 2 3)
      v (slice s 1)]
  (assign s nil)
  (assert (= v (bytes 2 3)) "rebinding the source leaves the slice"))
(println "  6. a slice is a value of its own")

(println "slice: ok")
