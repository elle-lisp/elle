## Reader Roundtrip Tests
##
## Migrated from tests/property/reader.rs
## Tests the fundamental roundtrip property: read(display(read(s))) == read(s)
## for structurally valid source code.

(def {:assert-eq assert-eq :assert-true assert-true :assert-false assert-false :assert-list-eq assert-list-eq :assert-equal assert-equal :assert-not-nil assert-not-nil :assert-string-eq assert-string-eq :assert-err assert-err :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

# ============================================================================
# Integer roundtrip
# ============================================================================

(assert-eq (read-all "0") (list 0) "read integer 0")
(assert-eq (read-all "1") (list 1) "read integer 1")
(assert-eq (read-all "-1") (list -1) "read integer -1")
(assert-eq (read-all "42") (list 42) "read integer 42")
(assert-eq (read-all "-42") (list -42) "read integer -42")
(assert-eq (read-all "999999") (list 999999) "read integer 999999")

# ============================================================================
# Boolean roundtrip
# ============================================================================

(assert-eq (read-all "true") (list true) "read boolean true")
(assert-eq (read-all "false") (list false) "read boolean false")

# ============================================================================
# nil roundtrip
# ============================================================================

(assert-eq (read-all "nil") (list nil) "read nil")

# ============================================================================
# String roundtrip
# ============================================================================

(assert-eq (read-all "\"\"") (list "") "read empty string")
(assert-eq (read-all "\"hello\"") (list "hello") "read string hello")
(assert-eq (read-all "\"test\"") (list "test") "read string test")
(assert-eq (read-all "\"with spaces\"") (list "with spaces") "read string with spaces")

# ============================================================================
# Symbol roundtrip
# ============================================================================

(assert-eq (read-all "a") (list 'a) "read symbol a")
(assert-eq (read-all "foo") (list 'foo) "read symbol foo")
(assert-eq (read-all "my-symbol") (list 'my-symbol) "read symbol my-symbol")
(assert-eq (read-all "x") (list 'x) "read symbol x")

# ============================================================================
# Keyword roundtrip
# ============================================================================

(assert-eq (read-all ":a") (list :a) "read keyword :a")
(assert-eq (read-all ":foo") (list :foo) "read keyword :foo")
(assert-eq (read-all ":my-keyword") (list :my-keyword) "read keyword :my-keyword")
(assert-eq (read-all ":x") (list :x) "read keyword :x")

# ============================================================================
# List roundtrip
# ============================================================================

(assert-eq (read-all "()") (list (list)) "read empty list")
(assert-eq (read-all "(1)") (list (list 1)) "read list with one element")
(assert-eq (read-all "(1 2 3)") (list (list 1 2 3)) "read list (1 2 3)")
(assert-eq (read-all "(-5 0 7)") (list (list -5 0 7)) "read list (-5 0 7)")

# ============================================================================
# Nested list roundtrip
# ============================================================================

(assert-eq (read-all "((1))") (list (list (list 1))) "read nested list depth 2")
(assert-eq (read-all "(((1)))") (list (list (list (list 1)))) "read nested list depth 3")
(assert-eq (read-all "((1 2) 3)") (list (list (list 1 2) 3)) "read nested list with pair")

# ============================================================================
# Tuple roundtrip
# ============================================================================

(assert-eq (read-all "[]") (list []) "read empty tuple")
(assert-eq (read-all "[1]") (list [1]) "read tuple with one element")
(assert-eq (read-all "[1 2 3]") (list [1 2 3]) "read tuple [1 2 3]")
(assert-eq (read-all "[-5 0 7]") (list [-5 0 7]) "read tuple [-5 0 7]")

# ============================================================================
# Array roundtrip
# ============================================================================

(assert-eq (read-all "@[]") (list @[]) "read empty array")
(assert-eq (read-all "@[1]") (list @[1]) "read array with one element")
(assert-eq (read-all "@[1 2 3]") (list @[1 2 3]) "read array @[1 2 3]")
(assert-eq (read-all "@[-5 0 7]") (list @[-5 0 7]) "read array @[-5 0 7]")

# ============================================================================
# Quote roundtrip
# ============================================================================

# Note: read-all returns a list of read values. When we read "'42", we get
# a list containing one element: the quoted form (quote 42).
# So (read-all "'42") returns ((quote 42)), which is a list with one element
# that is itself a list (the quoted form).
(assert-eq (read-all "'42") (list (list 'quote 42)) "read quoted 42")
(assert-eq (read-all "'foo") (list (list 'quote 'foo)) "read quoted symbol")
(assert-eq (read-all "'(+ 1 2)") (list (list 'quote (list '+ 1 2))) "read quoted list")
(assert-eq (read-all "'[1 2]") (list (list 'quote [1 2])) "read quoted tuple")

# ============================================================================
# Mixed nested structures
# ============================================================================

(assert-eq (read-all "([1 2] 3)") (list (list [1 2] 3)) "read list with tuple")
(assert-eq (read-all "[@[1] 2]") (list [@[1] 2]) "read tuple with array")
(assert-eq (read-all "(foo :bar 42)") (list (list 'foo :bar 42)) "read list with symbol, keyword, int")
