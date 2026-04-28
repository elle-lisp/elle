(elle/epoch 9)
## Bytes and @bytes Type Tests
##
## Tests for the immutable bytes and mutable @bytes types.
## Migrated from tests/integration/bytes.rs


# ============================================================================
# Bytes and blob constructors
# ============================================================================

(assert (bytes? (bytes 72 101 108 108 111)) "bytes constructor creates bytes")
(assert (bytes? (bytes)) "empty bytes constructor")

(assert (bytes? (@bytes 72 101 108 108 111)) "@bytes constructor creates @bytes")
(assert (bytes? (@bytes)) "empty @bytes constructor")

# ============================================================================
# Bytes and blob predicates
# ============================================================================

(assert (bytes? (bytes 1 2 3)) "bytes? true for bytes")
(assert (bytes? (@bytes 1 2 3)) "bytes? true for @bytes")

# ============================================================================
# String to bytes/blob conversions
# ============================================================================

(assert (bytes? (bytes "hello")) "bytes from string returns bytes")
(assert (bytes? (@bytes "hello")) "@bytes from string returns @bytes")

# ============================================================================
# Bytes/blob to string conversions
# ============================================================================

(assert (= (string (bytes 104 105)) "hi") "bytes->string")
(assert (= (freeze (string (@bytes 104 105))) "hi") "@bytes->string")

# ============================================================================
# Bytes/blob to hex conversions
# ============================================================================

(assert (= (bytes->hex (bytes 72 101 108)) "48656c") "bytes->hex")
(assert (= (freeze (bytes->hex (@bytes 72 101 108))) "48656c") "@bytes->hex")

# ============================================================================
# Bytes and blob length
# ============================================================================

(assert (= (length (bytes 1 2 3 4 5)) 5) "bytes length")
(assert (= (length (@bytes 1 2 3 4 5)) 5) "@bytes length")

# ============================================================================
# Bytes and blob get
# ============================================================================

(assert (= (get (bytes 72 101 108) 1) 101) "bytes get")
(let [[ok? _] (protect ((fn () (get (bytes 72 101 108) 10))))]
  (assert (not ok?) "bytes get out of bounds errors"))

(assert (= (get (@bytes 72 101 108) 1) 101) "@bytes get")
(let [[ok? _] (protect ((fn () (get (@bytes 72 101 108) 10))))]
  (assert (not ok?) "@bytes get out of bounds errors"))

# ============================================================================
# URI encoding
# ============================================================================

(assert (= (uri-encode "hello") "hello") "uri-encode simple")
(assert (= (uri-encode "hello world") "hello%20world") "uri-encode space")
(assert (= (uri-encode "a/b") "a%2Fb") "uri-encode special")

# ============================================================================
# @bytes mutations
# ============================================================================

(assert (bytes? (let [b (@bytes 1 2)]
                  (push b 3)
                  b))
        "@bytes push returns @bytes")
(assert (= (let [b (@bytes 1 2 3)]
             (pop b))
           3)
        "@bytes pop returns byte")
(assert (= (let [b (@bytes 1 2 3)]
             (put b 1 99)
             (get b 1))
           99)
        "@bytes put and get")

# ============================================================================
# Each over bytes and blob
# ============================================================================

(assert (= (let [@sum 0]
             (each b (bytes 1 2 3)
               (assign sum (+ sum b)))
             sum)
           6)
        "each over bytes")

(assert (= (let [@sum 0]
             (each b (@bytes 10 20 30)
               (assign sum (+ sum b)))
             sum)
           60)
        "each over @bytes")

# ============================================================================
# Map over bytes
# ============================================================================

(assert (= (length (map (fn (b) (* b 2)) (bytes 1 2 3))) 3)
        "map over bytes returns list")

# ============================================================================
# @string to bytes/@bytes conversions
# ============================================================================

(assert (bytes? (bytes (thaw "hello"))) "bytes from @string returns bytes")
(assert (bytes? (@bytes (thaw "hello"))) "@bytes from @string returns @bytes")

# ============================================================================
# Bytes/@bytes to @string conversions
# ============================================================================

(assert (= (string (bytes 104 105)) "hi") "bytes->string via (string)")
(assert (= (freeze (string (@bytes 104 105))) "hi")
        "@bytes->string via (string)")

# ============================================================================
# Mutability-preserving conversions
# ============================================================================

# (string x) coerces to immutable string regardless of input mutability
(assert (= (type (string (bytes 104 105))) :string)
        "string from bytes is immutable")
(assert (= (type (string (@bytes 104 105))) :string)
        "string from @bytes is immutable")

(assert (= (type (string "hello")) :string) "string from string is immutable")
(assert (= (type (string (thaw "hello"))) :string)
        "string from @string is immutable")

# (bytes x) preserves mutability: string→bytes, @string→@bytes
(assert (= (type (bytes "hello")) :bytes) "bytes from string is immutable")
(assert (= (type (bytes (thaw "hello"))) :@bytes)
        "bytes from @string is mutable")

# (bytes->hex x) preserves mutability: bytes→string, @bytes→@string
(assert (= (type (bytes->hex (bytes 72 101 108))) :string)
        "bytes->hex from bytes is immutable")
(assert (= (type (bytes->hex (@bytes 72 101 108))) :@string)
        "bytes->hex from @bytes is mutable")

# ============================================================================
# seq->hex — bytes input (same as bytes->hex, backward compat)
# ============================================================================

(assert (= (seq->hex (bytes 72 101 108)) "48656c") "seq->hex bytes")
(assert (= (freeze (seq->hex (@bytes 72 101 108))) "48656c")
        "seq->hex @bytes value")
(assert (= (type (seq->hex (@bytes 72 101 108))) :@string)
        "seq->hex @bytes is mutable")

# ============================================================================
# seq->hex — integer input (big-endian minimal bytes)
# ============================================================================

(assert (= (seq->hex 0) "00") "seq->hex zero")
(assert (= (seq->hex 255) "ff") "seq->hex 255")
(assert (= (seq->hex 256) "0100") "seq->hex 256")
(assert (= (seq->hex 65535) "ffff") "seq->hex 65535")
(let [[ok? err] (protect ((fn () (seq->hex -1))))]
  (assert (not ok?) "seq->hex negative int errors")
  (assert (= (get err :error) :value-error) "seq->hex negative int errors"))

# ============================================================================
# seq->hex — array input
# ============================================================================

(assert (= (seq->hex [72 101 108]) "48656c") "seq->hex array")
(assert (= (type (seq->hex [72 101 108])) :string) "seq->hex array is immutable")
(assert (= (freeze (seq->hex @[72 101 108])) "48656c") "seq->hex @array value")
(assert (= (type (seq->hex @[72 101 108])) :@string)
        "seq->hex @array is mutable")
(let [[ok? err] (protect ((fn () (seq->hex [256]))))]
  (assert (not ok?) "seq->hex array element out of range")
  (assert (= (get err :error) :value-error)
          "seq->hex array element out of range"))
(let [[ok? err] (protect ((fn () (seq->hex ["x"]))))]
  (assert (not ok?) "seq->hex array element not int")
  (assert (= (get err :error) :type-error) "seq->hex array element not int"))

# ============================================================================
# seq->hex — list input
# ============================================================================

(assert (= (seq->hex '(72 101 108)) "48656c") "seq->hex list")
(assert (= (type (seq->hex '(72 101 108))) :string) "seq->hex list is immutable")
(let [[ok? err] (protect ((fn () (seq->hex '(256)))))]
  (assert (not ok?) "seq->hex list element out of range")
  (assert (= (get err :error) :value-error) "seq->hex list element out of range"))
(let [[ok? err] (protect ((fn () (seq->hex '("x")))))]
  (assert (not ok?) "seq->hex list element not int")
  (assert (= (get err :error) :type-error) "seq->hex list element not int"))

# ============================================================================
# seq->hex — bytes->hex still works as alias
# ============================================================================

(assert (= (bytes->hex (bytes 72 101 108)) "48656c")
        "bytes->hex alias still works")
