(elle/epoch 18)
# audited: 2026-11-20
# A callee receives the value each operand held at the call, whatever runs before its body reads it.
# docs/impl/graft/operands.md
#
# The counter-factual: a compiler that wrote a captured binding in place of the
# callee's parameter read it after the owner wrote it, and answered 100. The
# same in-place reads refused a wrapper over such a binding at compile time.

# A later operand resumes the fiber where the owner of lo is parked, and the
# owner writes lo before the callee's body runs.
(defn first-of [x y]
  x)
(defn later-owner [@lo]
  (assign lo 0)
  (yield (fn [d] (first-of lo (d))))
  (assign lo 100)
  (yield nil))
(def later-parked (fiber/new (fn [] (later-owner 1)) |:yield|))
(def later-reader (fiber/resume later-parked))
(assert (= (later-reader (fn []
                           (fiber/resume later-parked)
                           2)) 0)
        "x is the 0 lo held before the second operand resumed the owner")

# The callee's own body resumes the owner before it reads its parameter.
(var body-parked nil)
(defn kick []
  (fiber/resume body-parked)
  2)
(defn after-kick [x]
  (kick)
  x)
(defn body-owner [@lo]
  (assign lo 0)
  (yield (fn [] (after-kick lo)))
  (assign lo 100)
  (yield nil))
(assign body-parked (fiber/new (fn [] (body-owner 1)) |:yield|))
(assert (= ((fiber/resume body-parked)) 0)
        "x is what lo held at the call, not what the owner wrote")

# A wrapper over a captured binding its owner writes, in either operand order.
(defn adder [@lo]
  (assign lo 1)
  (fn [d] (+ lo (d))))
(assert (= ((adder 0) (fn [] 2)) 3) "the captured binding is the first operand")
(defn adder-after [@lo]
  (assign lo 1)
  (fn [d] (+ (d) lo)))
(assert (= ((adder-after 0) (fn [] 2)) 3)
        "the captured binding is the second operand")

# A named parameter its default writes, captured by the closure the function
# returns: the shape of a generator that draws a length.
(defn sized [most &named @least]
  (default least 0)
  (fn [draw] (+ least (draw (+ (- most least) 1)))))
(assert (= ((sized 4 :least 1) (fn [n] (- n 1))) 4) "least plus the draw")
