(elle/epoch 9)
# Parameters — Racket-style dynamic bindings
#
# Tests for parameter, parameter?, parameterize, and fiber inheritance.


# === Basic parameter creation and predicates ===

(assert (parameter? (parameter 1)) "parameter? on parameter")
(assert (not (parameter? 42)) "parameter? on int")
(assert (not (parameter? "hello")) "parameter? on string")
(assert (not (parameter? (fn () 1))) "parameter? on closure")

# === Reading parameter values ===

(assert (= ((parameter 42)) 42) "call parameter reads default int")
(assert (= ((parameter "hello")) "hello") "call parameter reads default string")
(assert (= ((parameter nil)) nil) "call parameter reads default nil")

# === Parameter via def ===

(def p (parameter 99))
(assert (= (p) 99) "parameter via def reads default")

# === Parameterize basic override and revert ===

(def p1 (parameter 1))
(assert (= (parameterize ((p1 2))
             (p1))
           2)
        "parameterize overrides value")
(assert (= (p1) 1) "parameterize reverts after exit")

# === Parameterize with multiple expressions (body is begin) ===

(def p2 (parameter 0))
(assert (= (parameterize ((p2 42))
             (def x (p2))
             x)
           42)
        "parameterize body is begin")

# === Nested parameterize with shadowing ===

(def p3 (parameter 1))
(assert (= (parameterize ((p3 2))
             (parameterize ((p3 3))
               (p3)))
           3)
        "nested parameterize shadows outer")

# === Nested parameterize with outer visible after inner ===

(def p4 (parameter 1))
(assert (= (parameterize ((p4 2))
             (parameterize ((p4 3))
               (p4))
             (p4))
           2)
        "outer parameterize visible after inner exits")

# === Multiple bindings in one parameterize ===

(def a (parameter 1))
(def b (parameter 10))
(assert (= (parameterize ((a 2)
                          (b 20))
             (+ (a) (b)))
           22)
        "multiple bindings in parameterize")

# === Fiber inheritance ===

(def p5 (parameter 1))
(assert (= (parameterize ((p5 42))
             (let [f (fiber/new (fn () (p5)) 1)]
               (fiber/resume f nil)
               (fiber/value f)))
           42)
        "child fiber inherits parent parameterize")

# === Fiber inheritance outside parameterize ===

(def p6 (parameter 99))
(assert (= (let [f (fiber/new (fn () (p6)) 1)]
             (fiber/resume f nil)
             (fiber/value f))
           99)
        "child fiber sees parent default outside parameterize")

# ============================================================================
# Type and error tests (from integration/parameters.rs)
# ============================================================================

# make_parameter_returns_parameter
(assert (parameter? (parameter 42)) "parameter returns parameter type")

# parameter_type_of
(assert (= (type (parameter 0)) :parameter) "type-of parameter is :parameter")

# parameter_call_with_args_errors
(let [[ok? _] (protect ((fn () ((parameter 42) 1))))]
  (assert (not ok?) "parameter call with args errors"))

# parameterize_non_parameter_errors
(let [[ok? _] (protect ((fn ()
                          (eval '(parameterize ((42 1))
                                   0)))))]
  (assert (not ok?) "parameterize with non-parameter errors"))
