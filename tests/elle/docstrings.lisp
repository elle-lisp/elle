# Docstrings — documentation extraction and retrieval
#
# Tests for the (doc name) primitive and docstring extraction from function definitions.
# Migrated from tests/integration/docstrings.rs.

(import-file "tests/elle/assert.lisp")

# === User-defined docstrings ===

# Docstring in fn form
(def my-fn (fn (x) "Adds one to x" (+ x 1)))
(assert-eq (doc "my-fn") "Adds one to x"
  "fn with docstring should extract it")

# Docstring in defn macro
(defn greet (name) "Greets someone by name" (string/append "Hello, " name))
(assert-eq (doc "greet") "Greets someone by name"
  "defn with docstring should extract it")

# === Builtin docstrings ===

# Builtin primitives have documentation
(let ((doc-str (doc "+")))
  (assert-true (string/contains? doc-str "+")
    "Builtin + should have documentation containing '+'"))

# === Missing docstrings ===

# Undefined variable returns "No documentation found"
(let ((result (doc "undefined-var-xyz")))
  (assert-true (string/contains? result "No documentation found")
    "Undefined variable should return 'No documentation found'"))

# Variable without docstring returns "No documentation found"
(def x 42)
(let ((result (doc "x")))
  (assert-true (string/contains? result "No documentation found")
    "Variable without docstring should return 'No documentation found'"))

# === Edge cases ===

# Single-body string is NOT a docstring (it's the return value)
(def single-string-fn (fn () "hello"))
(let ((result (doc "single-string-fn")))
  (assert-true (string/contains? result "No documentation found")
    "Single-body string should not be treated as docstring"))

# Docstring with multiple body expressions
(def multi-body-fn (fn (x y) "Adds two numbers" (+ x y)))
(assert-eq (doc "multi-body-fn") "Adds two numbers"
  "Docstring with multiple body expressions should work")

# Docstring with complex body
(def complex-fn (fn (n)
  "Computes factorial"
  (if (<= n 1)
    1
    (* n (complex-fn (- n 1))))))
(assert-eq (doc "complex-fn") "Computes factorial"
  "Docstring with complex body should work")

# Empty docstring
(def empty-doc-fn (fn () "" 42))
(assert-eq (doc "empty-doc-fn") ""
  "Empty docstring should be preserved")
