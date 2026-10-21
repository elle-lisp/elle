(elle/epoch 15)
# audited: 2026-10-20
## bytes/find answers the byte index of a needle in bytes or a string, from an optional offset, or nil.
## docs/bytes.md
##
## The traps: the index is a byte offset where `string/find` answers a
## grapheme index; an integer needle is one byte; an empty needle is found at
## the offset; an offset past the end or below zero answers nil rather than
## an error.

(def lines (bytes "a\nbc\n"))

(assert (= (bytes/find lines (bytes "\n")) 1) "the first newline")
(assert (= (bytes/find lines 10) 1) "an integer needle is one byte")
(assert (= (bytes/find lines "\n" 2) 4) "the search starts at the offset")
(assert (= (bytes/find lines "bc") 2) "a needle of several bytes")
(assert (= (bytes/find "héllo" "l") 3) "a byte index, not a grapheme index")
(assert (= (bytes/find (@bytes "abc") "c") 2) "an @bytes haystack")
(assert (= (bytes/find @"abc" (@bytes "b")) 1)
        "an @string haystack and an @bytes needle")
(assert (= (bytes/find (slice lines 1) "\n") 0) "a haystack that is a slice")
(assert (= (bytes/find "abcabc" "bc" 2) 4)
        "the first occurrence at or after the offset")
(println "  1. found")

(assert (nil? (bytes/find "ab" "\n")) "nil when the needle is absent")
(assert (nil? (bytes/find "ab" "abc"))
        "nil when the needle is longer than the haystack")
(assert (nil? (bytes/find "ab" "a" 1))
        "nil when the needle lies before the offset")
(assert (nil? (bytes/find "ab" "b" 3)) "nil when the offset is past the end")
(assert (nil? (bytes/find "ab" "b" -1)) "nil for a negative offset")
(println "  2. not found")

(assert (= (bytes/find "abc" "" 1) 1) "an empty needle is found at the offset")
(assert (= (bytes/find "abc" "" 3) 3) "and at the end")
(assert (= (bytes/find (bytes) "" 0) 0) "and in an empty haystack")
(println "  3. an empty needle")

(let [[ok? err] (protect (bytes/find 1 "a"))]
  (assert (not ok?) "a haystack must be bytes or a string")
  (assert (= (get err :error) :type-error)))
(let [[ok? err] (protect (bytes/find "a" 1.5))]
  (assert (not ok?) "a needle must be bytes, a string or a byte")
  (assert (= (get err :error) :type-error)))
(let [[ok? err] (protect (bytes/find "a" 300))]
  (assert (not ok?) "an integer needle must be one byte")
  (assert (= (get err :error) :value-error)))
(let [[ok? err] (protect (bytes/find "a" "a" "0"))]
  (assert (not ok?) "an offset must be an integer")
  (assert (= (get err :error) :type-error)))
(println "  4. errors")

(println "bytes-find: ok")
