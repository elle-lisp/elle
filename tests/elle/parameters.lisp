(elle/epoch 10)
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
             (p1)) 2) "parameterize overrides value")
(assert (= (p1) 1) "parameterize reverts after exit")

# === Parameterize with multiple expressions (body is begin) ===

(def p2 (parameter 0))
(assert (= (parameterize ((p2 42))
             (def x (p2))
             x) 42) "parameterize body is begin")

# === Nested parameterize with shadowing ===

(def p3 (parameter 1))
(assert (= (parameterize ((p3 2))
             (parameterize ((p3 3))
               (p3))) 3) "nested parameterize shadows outer")

# === Nested parameterize with outer visible after inner ===

(def p4 (parameter 1))
(assert (= (parameterize ((p4 2))
             (parameterize ((p4 3))
               (p4))
             (p4)) 2) "outer parameterize visible after inner exits")

# === Multiple bindings in one parameterize ===

(def a (parameter 1))
(def b (parameter 10))
(assert (= (parameterize ((a 2)
                          (b 20))
             (+ (a) (b))) 22) "multiple bindings in parameterize")

# === Fiber inheritance ===

(def p5 (parameter 1))
(assert (= (parameterize ((p5 42))
             (let [f (fiber/new (fn () (p5)) 1)]
               (fiber/resume f nil)
               (fiber/value f))) 42) "child fiber inherits parent parameterize")

# === Fiber inheritance outside parameterize ===

(def p6 (parameter 99))
(assert (= (let [f (fiber/new (fn () (p6)) 1)]
             (fiber/resume f nil)
             (fiber/value f)) 99)
        "child fiber sees parent default outside parameterize")

# === Fiber captures parameter at CREATION, not at first resume ===
#
# Regression: previously, parameter inheritance happened on first resume
# from the resuming fiber, not at fiber creation. That broke ev/spawn
# style usage where the spawner finishes (and its parameterize unwinds)
# before the scheduler ever gets around to resuming the child. Repro:
# create the fiber inside parameterize, then exit the parameterize block
# before resuming. The child must still see the parameterized value.

(def p7 (parameter :default))
(let [f (parameterize ((p7 :inside))
          (fiber/new (fn () (p7)) 1))]
  # parameterize has unwound — p7 is back to :default here
  (assert (= (p7) :default) "p7 is :default outside parameterize")
  (fiber/resume f nil)
  (assert (= (fiber/value f) :inside)
          "fiber sees :inside (creation-time snapshot), not :default"))

# === Creation snapshot is independent of resumer's bindings ===
#
# The fiber's snapshot must NOT be overridden by whatever bindings the
# resuming fiber happens to have when fiber/resume is called.

(def p8 (parameter :default))
(let [f (parameterize ((p8 :captured))
          (fiber/new (fn () (p8)) 1))]
  (parameterize ((p8 :resumer-has-this))
    (fiber/resume f nil))
  (assert (= (fiber/value f) :captured)
          "fiber observes creation-time value, ignoring resumer's binding"))

# === Nested parameterize: child captures innermost value at creation ===

(def p9 (parameter 1))
(let [f (parameterize ((p9 2))
          (parameterize ((p9 3))
            (fiber/new (fn () (p9)) 1)))]
  (fiber/resume f nil)
  (assert (= (fiber/value f) 3)
          "child fiber captures innermost parameterize binding"))

# === Multiple parameters: all are captured ===

(def pa (parameter :pa-default))
(def pb (parameter :pb-default))
(let [f (parameterize ((pa :pa-val)
                       (pb :pb-val))
          (fiber/new (fn () (list (pa) (pb))) 1))]
  (fiber/resume f nil)
  (let [r (fiber/value f)]
    (assert (= (get r 0) :pa-val) "fiber sees pa binding")
    (assert (= (get r 1) :pb-val) "fiber sees pb binding")))

# === Child of a child inherits transitively ===
#
# A fiber created inside an already-running fiber should snapshot the
# inner fiber's view of parameters, which in turn was snapshotted from
# the outer's view at creation.

(def pc (parameter :default))
(let [outer (parameterize ((pc :outer-set))
              (fiber/new (fn ()
                           (let [inner (fiber/new (fn () (pc)) 1)]
                             (fiber/resume inner nil)
                             (fiber/value inner))) 1))]
  (fiber/resume outer nil)
  (assert (= (fiber/value outer) :outer-set)
          "grandchild fiber sees grandparent's parameterize binding"))

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
