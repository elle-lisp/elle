# Parameters — Racket-style dynamic bindings
#
# Tests for make-parameter, parameter?, parameterize, and fiber inheritance.

(import-file "./examples/assertions.lisp")

# === Basic parameter creation and predicates ===

(assert-true (parameter? (make-parameter 1)) "parameter? on parameter")
(assert-false (parameter? 42) "parameter? on int")
(assert-false (parameter? "hello") "parameter? on string")
(assert-false (parameter? (fn () 1)) "parameter? on closure")

# === Reading parameter values ===

(assert-eq ((make-parameter 42)) 42 "call parameter reads default int")
(assert-eq ((make-parameter "hello")) "hello" "call parameter reads default string")
(assert-eq ((make-parameter nil)) nil "call parameter reads default nil")

# === Parameter via def ===

(def p (make-parameter 99))
(assert-eq (p) 99 "parameter via def reads default")

# === Parameterize basic override and revert ===

(def p1 (make-parameter 1))
(assert-eq (parameterize ((p1 2)) (p1)) 2 "parameterize overrides value")
(assert-eq (p1) 1 "parameterize reverts after exit")

# === Parameterize with multiple expressions (body is begin) ===

(def p2 (make-parameter 0))
(assert-eq
  (parameterize ((p2 42))
    (def x (p2))
    x)
  42
  "parameterize body is begin")

# === Nested parameterize with shadowing ===

(def p3 (make-parameter 1))
(assert-eq
  (parameterize ((p3 2))
    (parameterize ((p3 3))
      (p3)))
  3
  "nested parameterize shadows outer")

# === Nested parameterize with outer visible after inner ===

(def p4 (make-parameter 1))
(assert-eq
  (parameterize ((p4 2))
    (parameterize ((p4 3))
      (p4))
    (p4))
  2
  "outer parameterize visible after inner exits")

# === Multiple bindings in one parameterize ===

(def a (make-parameter 1))
(def b (make-parameter 10))
(assert-eq
  (parameterize ((a 2) (b 20))
    (+ (a) (b)))
  22
  "multiple bindings in parameterize")

# === Fiber inheritance ===

(def p5 (make-parameter 1))
(assert-eq
  (parameterize ((p5 42))
    (let ((f (fiber/new (fn () (p5)) 1)))
      (fiber/resume f nil)
      (fiber/value f)))
  42
  "child fiber inherits parent parameterize")

# === Fiber inheritance outside parameterize ===

(def p6 (make-parameter 99))
(assert-eq
  (let ((f (fiber/new (fn () (p6)) 1)))
    (fiber/resume f nil)
    (fiber/value f))
  99
  "child fiber sees parent default outside parameterize")
