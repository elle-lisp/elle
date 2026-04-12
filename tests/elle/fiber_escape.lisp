## Fiber escape analysis: shared allocator is installed only when needed.
##
## Tests that child fibers correctly handle heap values that escape to
## the parent via return, yield, or outward mutation — while still
## allowing scope allocation for fibers that return immediates.

# ============================================================================
# Fiber returning immediate — no shared alloc needed, scope alloc works
# ============================================================================

# Return an integer (immediate)
(let [[f (fiber/new (fn [] 42) |:yield|)]]
  (assert (= (fiber/resume f) 42) "fiber returning int"))

# Return a boolean (immediate)
(let [[f (fiber/new (fn [] true) |:yield|)]]
  (assert (= (fiber/resume f) true) "fiber returning bool"))

# Return nil (immediate)
(let [[f (fiber/new (fn [] nil) |:yield|)]]
  (assert (nil? (fiber/resume f)) "fiber returning nil"))

# Return a keyword (immediate)
(let [[f (fiber/new (fn [] :done) |:yield|)]]
  (assert (= (fiber/resume f) :done) "fiber returning keyword"))

# ============================================================================
# Fiber returning heap value — shared alloc needed, value survives
# ============================================================================

# Return a string (heap)
(let [[f (fiber/new (fn [] "hello world") |:yield|)]]
  (let [[result (fiber/resume f)]]
    (assert (= result "hello world") "fiber returning string")))

# Return a list (heap)
(let [[f (fiber/new (fn [] '(1 2 3)) |:yield|)]]
  (let [[result (fiber/resume f)]]
    (assert (= (length result) 3) "fiber returning list length")
    (assert (= (first result) 1) "fiber returning list first")))

# Return an array (heap)
(let [[f (fiber/new (fn [] [10 20 30]) |:yield|)]]
  (let [[result (fiber/resume f)]]
    (assert (= (length result) 3) "fiber returning array length")
    (assert (= (get result 0) 10) "fiber returning array element")))

# Return a struct (heap)
(let [[f (fiber/new (fn [] {:a 1 :b 2}) |:yield|)]]
  (let [[result (fiber/resume f)]]
    (assert (= (get result :a) 1) "fiber returning struct field")))

# ============================================================================
# Fiber with yield — shared alloc needed for yielded values
# ============================================================================

# Yield a string, return int
(let [[f (fiber/new (fn [] (yield "yielded") 99) |:yield|)]]
  (let [[y (fiber/resume f)]]
    (assert (= y "yielded") "fiber yield string")
    (assert (= (fiber/resume f) 99) "fiber return after yield")))

# Yield an array
(let [[f (fiber/new (fn [] (yield [1 2 3]) :done) |:yield|)]]
  (let [[y (fiber/resume f)]]
    (assert (= (length y) 3) "fiber yield array length")
    (assert (= (get y 0) 1) "fiber yield array element")))

# ============================================================================
# Fiber with outward mutation of heap value
# ============================================================================

# Captured mutable variable set to heap value inside fiber
(var box nil)
(let [[f (fiber/new (fn [] (assign box "escaped") 42) |:yield|)]]
  (fiber/resume f)
  (assert (= box "escaped") "fiber outward mutation string"))

# Captured mutable variable set to array inside fiber
(var holder nil)
(let [[f (fiber/new (fn [] (assign holder [1 2 3]) :ok) |:yield|)]]
  (fiber/resume f)
  (assert (= (length holder) 3) "fiber outward mutation array length")
  (assert (= (get holder 1) 2) "fiber outward mutation array element"))

# ============================================================================
# Non-literal lambda passed to fiber/new
# ============================================================================

# defn at top-level, passed as variable
(defn make-string [] "from-defn")

(let [[f (fiber/new make-string |:yield|)]]
  (let [[result (fiber/resume f)]]
    (assert (= result "from-defn") "fiber with defn lambda")))

# Lambda stored in a variable
(let* [[thunk (fn [] {:key "value"})]
       [f (fiber/new thunk |:yield|)]]
  (let [[result (fiber/resume f)]]
    (assert (= (get result :key) "value") "fiber with variable lambda")))

# ============================================================================
# Long-lived reference to fiber return value after handle dropped
# ============================================================================

(def escaped-value
  (let [[f (fiber/new (fn [] "long-lived") |:yield|)]]
    (fiber/resume f)))

# The fiber handle `f` is out of scope; the returned string must survive
(assert (= escaped-value "long-lived") "value survives fiber handle drop")

# Same for a complex value
(def escaped-struct
  (let [[f (fiber/new (fn [] {:x 1 :y [2 3]}) |:yield|)]]
    (fiber/resume f)))

(assert (= (get escaped-struct :x) 1) "escaped struct field")
(assert (= (get (get escaped-struct :y) 0) 2) "escaped struct nested array")

# ============================================================================
# Multiple resumes with heap values
# ============================================================================

(let [[f (fiber/new (fn []
                      (yield "first")
                      (yield "second")
                      "third")
                    |:yield|)]]
  (assert (= (fiber/resume f) "first") "multi-yield first")
  (assert (= (fiber/resume f) "second") "multi-yield second")
  (assert (= (fiber/resume f) "third") "multi-yield final"))
