(elle/epoch 9)
## String and @string Type Tests
##
## Tests for immutable string and mutable @string types.
## @string constructor and operations, string operations on @strings.
## Migrated from tests/integration/buffer.rs


# ============================================================================
# @string literals and constructors
# ============================================================================

(assert (string? (thaw "hello")) "@string literal is @string")
(assert (string? (thaw "")) "empty @string literal is @string")
(assert (string? (@string)) "@string constructor creates @string")
(assert (string? (@string 72 101 108 108 111)) "@string with bytes is @string")

# ============================================================================
# @string predicates
# ============================================================================

(assert (string? (thaw "hello")) "string? true for @string")
(assert (string? "hello") "string? true for string")

# ============================================================================
# @string length
# ============================================================================

(assert (= (length (thaw "hello")) 5) "length of @\"hello\"")
(assert (= (length (thaw "")) 0) "length of empty buffer")

# ============================================================================
# @string empty predicate
# ============================================================================

(assert (empty? (thaw "")) "empty? true for empty @string")
(assert (not (empty? (thaw "hello"))) "empty? false for non-empty @string")

# ============================================================================
# @string get
# ============================================================================

# @string get returns grapheme cluster as string, not byte
(assert (= (get (thaw "hello") 0) "h") "get @\"hello\" 0")
(assert (= (get (thaw "hello") 2) "l") "get @\"hello\" 2")
(assert (= (get (thaw "hello") 4) "o") "get @\"hello\" 4")
(assert (= (get (thaw "hello") 100) nil) "get out of bounds returns nil")
(assert (= (get (thaw "hello") 100 99) 99) "get with default")

# ============================================================================
# @string put
# ============================================================================

(let [[ok? _] (protect ((fn () (put (thaw "hello") 10 "x"))))]
  (assert (not ok?) "put out of bounds errors"))
(assert (= (freeze (begin
                     (def @s (thaw "hello"))
                     (put s -1 "x")
                     s)) "hellx") "put negative index wraps")
(let [[ok? _] (protect ((fn () (put (thaw "") 0 "x"))))]
  (assert (not ok?) "put on empty @string errors"))

# ============================================================================
# @string grapheme-consistent indexing
# ============================================================================

# length counts grapheme clusters, not bytes
(assert (= (length (thaw "café")) 4)
  "length @\"café\" is 4 graphemes, not 5 bytes")
(assert (= (length (thaw "🎉🎊")) 2)
  "length of emoji @string is grapheme count")
(assert (= (length (thaw "naïve")) 5)
  "length @\"naïve\" counts combining sequence as one grapheme")

# put replaces grapheme cluster at the given position
(let [s (thaw "café")]
  (put s 3 "E")
  (assert (= (freeze s) "cafE") "put replaces grapheme at index 3"))

(let [s (thaw "hello")]
  (put s 0 "H")
  (assert (= (freeze s) "Hello") "put replaces first grapheme"))

(let [s (thaw "hello")]
  (put s 4 "O")
  (assert (= (freeze s) "hellO") "put replaces last grapheme"))

# put accepts multi-byte replacement string
(let [s (thaw "cafe")]
  (put s 3 "é")
  (assert (= (freeze s) "café") "put can replace with multi-byte grapheme"))

# round-trip: get then put restores original
(let [s (thaw "café")]
  (let [g (get s 3)]
    (put s 3 "E")
    (put s 3 g)
    (assert (= (freeze s) "café") "get/put round-trip preserves original value")))

# put type errors
(let [[ok? _] (protect ((fn () (put (thaw "hello") 0 88))))]
  (assert (not ok?) "put @string rejects integer value"))
(let [[ok? _] (protect ((fn () (put (thaw "hello") "a" "b"))))]
  (assert (not ok?) "put @string rejects non-integer index"))

# put bounds errors (use string values, matching new semantics)
(let [[ok? _] (protect ((fn () (put (thaw "hello") 10 "x"))))]
  (assert (not ok?) "put out of bounds errors (new)"))
(assert (= (freeze (begin
                     (def @s2 (thaw "hello"))
                     (put s2 -1 "x")
                     s2)) "hellx") "put negative index wraps (new)")
(let [[ok? _] (protect ((fn () (put (thaw "") 0 "x"))))]
  (assert (not ok?) "put on empty @string errors (new)"))

# ============================================================================
# @string pop
# ============================================================================

(assert (= (begin
             (def @b (thaw "hi"))
             (pop b)) "i") "pop returns last grapheme as string")
(assert (= (begin
             (def @b (thaw "café"))
             (pop b)) "é") "pop returns last multibyte grapheme")
(let [[ok? _] (protect ((fn ()
                          (begin
                            (def @b (thaw ""))
                            (pop b)))))]
  (assert (not ok?) "pop on empty @string errors"))

# ============================================================================
# @string push
# ============================================================================

(assert (string? (begin
                   (def @b (thaw "hi"))
                   (push b "!")
                   b)) "push returns @string")
(assert (= (freeze (begin
                     (def @b (thaw "hi"))
                     (push b "!")
                     b)) "hi!") "push appends string to @string")
(assert (= (freeze (begin
                     (def @b (thaw "café"))
                     (push b "x")
                     b)) "caféx") "push appends to multibyte @string")
(let [[ok? _] (protect ((fn ()
                          (begin
                            (def @b (thaw "hi"))
                            (push b 33)))))]
  (assert (not ok?) "push rejects integer for @string"))

# ============================================================================
# @string append
# ============================================================================

(assert (string? (begin
                   (def @b (thaw "hello"))
                   (append b (thaw " world"))
                   b)) "append returns @string")

# ============================================================================
# @string concat
# ============================================================================

(assert (string? (concat (thaw "hello") (thaw " world")))
  "concat returns @string")

# ============================================================================
# @string roundtrip conversions
# ============================================================================

(assert (= (freeze (thaw "hello")) "hello") "freeze/thaw string roundtrip")
(assert (= (freeze (thaw "hello")) "hello") "freeze @string literal")

# ============================================================================
# @string insert
# ============================================================================

(assert (string? (begin
                   (def @b (thaw "hllo"))
                   (insert b 1 101)
                   b)) "insert returns @string")

# ============================================================================
# @string remove
# ============================================================================

(assert (string? (begin
                   (def @b (thaw "hello"))
                   (remove b 1)
                   b)) "remove returns @string")
(assert (string? (begin
                   (def @b (thaw "hello"))
                   (remove b 1 2)
                   b)) "remove multiple returns @string")

# ============================================================================
# @string popn
# ============================================================================

(assert (string? (begin
                   (def @b (thaw "hello"))
                   (popn b 2))) "popn returns @string")

# ============================================================================
# String operations on @strings
# ============================================================================

(assert (string/contains? (thaw "hello world") "world")
  "@string contains substring")
(assert (not (string/contains? (thaw "hello") "xyz"))
  "@string doesn't contain substring")

(assert (string/starts-with? (thaw "hello") "he") "@string starts with prefix")
(assert (not (string/starts-with? (thaw "hello") "lo"))
  "@string doesn't start with suffix")

(assert (string/ends-with? (thaw "hello") "lo") "@string ends with suffix")
(assert (not (string/ends-with? (thaw "hello") "he"))
  "@string doesn't end with prefix")

(assert (= (string/index (thaw "hello") "l") 2) "@string index of substring")
(assert (= (string/index (thaw "hello") "z") nil) "@string index not found")

(assert (= (freeze (slice (thaw "hello") 1 4)) "ell") "slice of @string")

(assert (string? (string/upcase (thaw "hello")))
  "upcase @string returns @string")
(assert (string? (string/downcase (thaw "HELLO")))
  "downcase @string returns @string")

(assert (string? (string/trim (thaw "  hello  ")))
  "trim @string returns @string")

(assert (= (get (thaw "hello") 1) "e") "get on @string returns e")

# ============================================================================
# @string split
# ============================================================================

(assert (= (length (string/split (thaw "a,b,c") ",")) 3)
  "split @string returns 3 parts")

# ============================================================================
# @string replace
# ============================================================================

(assert (string? (string/replace (thaw "hello") "l" "L"))
  "replace on @string returns @string")

# ============================================================================
# Concat on lists
# ============================================================================

(assert (= (length (concat (list 1 2) (list 3 4))) 4) "concat lists length")
(assert (= (length (concat (list) (list 1 2))) 2) "concat with empty list")

# Verify original lists unchanged
(assert (= (let [a (list 1 2)]
             (let [b (concat a (list 3 4))]
               (list (length a) (length b)))) (list 2 4))
  "concat doesn't modify original lists")
