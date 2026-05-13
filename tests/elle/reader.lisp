(elle/epoch 11)
## Reader Roundtrip Tests
##
## Migrated from tests/property/reader.rs
## Tests the fundamental roundtrip property: read(display(read(s))) == read(s)
## for structurally valid source code.


# ============================================================================
# Integer roundtrip
# ============================================================================

(assert (= (read-all "0") (list 0)) "read integer 0")
(assert (= (read-all "1") (list 1)) "read integer 1")
(assert (= (read-all "-1") (list -1)) "read integer -1")
(assert (= (read-all "42") (list 42)) "read integer 42")
(assert (= (read-all "-42") (list -42)) "read integer -42")
(assert (= (read-all "999999") (list 999999)) "read integer 999999")

# ============================================================================
# Boolean roundtrip
# ============================================================================

(assert (= (read-all "true") (list true)) "read boolean true")
(assert (= (read-all "false") (list false)) "read boolean false")

# ============================================================================
# nil roundtrip
# ============================================================================

(assert (= (read-all "nil") (list nil)) "read nil")

# ============================================================================
# String roundtrip
# ============================================================================

(assert (= (read-all "\"\"") (list "")) "read empty string")
(assert (= (read-all "\"hello\"") (list "hello")) "read string hello")
(assert (= (read-all "\"test\"") (list "test")) "read string test")
(assert (= (read-all "\"with spaces\"") (list "with spaces"))
        "read string with spaces")

# ============================================================================
# Symbol roundtrip
# ============================================================================

(assert (= (read-all "a") (list 'a)) "read symbol a")
(assert (= (read-all "foo") (list 'foo)) "read symbol foo")
(assert (= (read-all "my-symbol") (list 'my-symbol)) "read symbol my-symbol")
(assert (= (read-all "x") (list 'x)) "read symbol x")

# ============================================================================
# Keyword roundtrip
# ============================================================================

(assert (= (read-all ":a") (list :a)) "read keyword :a")
(assert (= (read-all ":foo") (list :foo)) "read keyword :foo")
(assert (= (read-all ":my-keyword") (list :my-keyword))
        "read keyword :my-keyword")
(assert (= (read-all ":x") (list :x)) "read keyword :x")

# ============================================================================
# List roundtrip
# ============================================================================

(assert (= (read-all "()") (list (list))) "read empty list")
(assert (= (read-all "(1)") (list (list 1))) "read list with one element")
(assert (= (read-all "(1 2 3)") (list (list 1 2 3))) "read list (1 2 3)")
(assert (= (read-all "(-5 0 7)") (list (list -5 0 7))) "read list (-5 0 7)")

# ============================================================================
# Nested list roundtrip
# ============================================================================

(assert (= (read-all "((1))") (list (list (list 1)))) "read nested list depth 2")
(assert (= (read-all "(((1)))") (list (list (list (list 1)))))
        "read nested list depth 3")
(assert (= (read-all "((1 2) 3)") (list (list (list 1 2) 3)))
        "read nested list with pair")

# ============================================================================
# Tuple roundtrip
# ============================================================================

(assert (= (read-all "[]") (list [])) "read empty tuple")
(assert (= (read-all "[1]") (list [1])) "read tuple with one element")
(assert (= (read-all "[1 2 3]") (list [1 2 3])) "read tuple [1 2 3]")
(assert (= (read-all "[-5 0 7]") (list [-5 0 7])) "read tuple [-5 0 7]")

# ============================================================================
# Array roundtrip
# ============================================================================

(assert (= (read-all "@[]") (list @[])) "read empty array")
(assert (= (read-all "@[1]") (list @[1])) "read array with one element")
(assert (= (read-all "@[1 2 3]") (list @[1 2 3])) "read array @[1 2 3]")
(assert (= (read-all "@[-5 0 7]") (list @[-5 0 7])) "read array @[-5 0 7]")

# ============================================================================
# Quote roundtrip
# ============================================================================

# Note: read-all returns a list of read values. When we read "'42", we get
# a list containing one element: the quoted form (quote 42).
# So (read-all "'42") returns ((quote 42)), which is a list with one element
# that is itself a list (the quoted form).
(assert (= (read-all "'42") (list (list 'quote 42))) "read quoted 42")
(assert (= (read-all "'foo") (list (list 'quote 'foo))) "read quoted symbol")
(assert (= (read-all "'(+ 1 2)") (list (list 'quote (list '+ 1 2))))
        "read quoted list")
(assert (= (read-all "'[1 2]") (list (list 'quote [1 2]))) "read quoted tuple")

# ============================================================================
# Mixed nested structures
# ============================================================================

(assert (= (read-all "([1 2] 3)") (list (list [1 2] 3))) "read list with tuple")
(assert (= (read-all "[@[1] 2]") (list [@[1] 2])) "read tuple with array")
(assert (= (read-all "(foo :bar 42)") (list (list 'foo :bar 42)))
        "read list with symbol, keyword, int")

# ──────────────────────────────────────────────────────────────────────────────
# @string roundtrip — read produces mutable string, eval preserves mutability
# ──────────────────────────────────────────────────────────────────────────────

(assert (= (type-of (read "@\"\"")) :@string) "read @\"\" produces @string")
(assert (= (type-of (read "@\"hello\"")) :@string)
        "read @\"hello\" produces @string")

(let [s (eval (read "@\"\""))]
  (push s "hello")
  (assert (= s @"hello") "eval of read @\"\" is mutable"))

(let [s (eval (read "@\"abc\""))]
  (push s "d")
  (assert (= s @"abcd") "eval of read @\"abc\" is mutable"))

# ──────────────────────────────────────────────────────────────────────────────
# @array roundtrip — read produces mutable array, eval preserves mutability
# ──────────────────────────────────────────────────────────────────────────────

(assert (= (type-of (read "@[]")) :@array) "read @[] produces @array")
(assert (= (type-of (read "@[1 2 3]")) :@array) "read @[1 2 3] produces @array")

(let [a (eval (read "@[]"))]
  (push a 1)
  (assert (= a @[1]) "eval of read @[] is mutable"))

(let [a (eval (read "@[1 2]"))]
  (push a 3)
  (assert (= a @[1 2 3]) "eval of read @[1 2] is mutable"))

# ──────────────────────────────────────────────────────────────────────────────
# bytes / @bytes — read desugars to call form, eval produces correct type
# ──────────────────────────────────────────────────────────────────────────────

(assert (= (read "b[1 2 3]") '(bytes 1 2 3))
        "read b[1 2 3] desugars to (bytes ...)")
(assert (= (read "@b[1 2 3]") '(@bytes 1 2 3))
        "read @b[1 2 3] desugars to (@bytes ...)")

(let [b (eval (read "b[1 2 3]"))]
  (assert (= (type-of b) :bytes) "eval of read b[...] produces bytes")
  (assert (= (get b 0) 1) "bytes element access after read/eval"))

(let [b (eval (read "@b[1 2 3]"))]
  (assert (= (type-of b) :@bytes) "eval of read @b[...] produces @bytes")
  (push b 4)
  (assert (= (get b 3) 4) "eval of read @b[...] is mutable"))

# ──────────────────────────────────────────────────────────────────────────────
# set / @set — read desugars to call form, eval produces correct type
# ──────────────────────────────────────────────────────────────────────────────

(assert (= (read "|1 2 3|") '(set 1 2 3)) "read |1 2 3| desugars to (set ...)")
(assert (= (read "@|1 2 3|") '(@set 1 2 3))
        "read @|1 2 3| desugars to (@set ...)")

(let [s (eval (read "|1 2 3|"))]
  (assert (= (type-of s) :set) "eval of read |...| produces set")
  (assert (contains? s 2) "set membership after read/eval"))

(let [s (eval (read "@|1 2|"))]
  (assert (= (type-of s) :@set) "eval of read @|...| produces @set")
  (add s 3)
  (assert (contains? s 3) "eval of read @|...| is mutable"))

# ──────────────────────────────────────────────────────────────────────────────
# struct / @struct — read desugars to call form, eval produces correct type
# ──────────────────────────────────────────────────────────────────────────────

(assert (= (type-of (eval (read "{:a 1}"))) :struct)
        "eval of read {...} produces struct")

(let [m (eval (read "@{:a 1}"))]
  (assert (= (type-of m) :@struct) "eval of read @{...} produces @struct")
  (put m :b 2)
  (assert (= (get m :b) 2) "eval of read @{...} is mutable"))
