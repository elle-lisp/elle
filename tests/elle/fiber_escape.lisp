(elle/epoch 7)
## Fiber escape analysis: shared allocator is installed only when needed.
##
## Tests that child fibers correctly handle heap values that escape to
## the parent via return, yield, or outward mutation — while still
## allowing scope allocation for fibers that return immediates.

# ============================================================================
# Fiber returning immediate — no shared alloc needed, scope alloc works
# ============================================================================

# Return an integer (immediate)
(let [[f (fiber/new |:yield| (fn [] 42))]]
  (assert (= (fiber/resume f) 42) "fiber returning int"))

# Return a boolean (immediate)
(let [[f (fiber/new |:yield| (fn [] true))]]
  (assert (= (fiber/resume f) true) "fiber returning bool"))

# Return nil (immediate)
(let [[f (fiber/new |:yield| (fn [] nil))]]
  (assert (nil? (fiber/resume f)) "fiber returning nil"))

# Return a keyword (immediate)
(let [[f (fiber/new |:yield| (fn [] :done))]]
  (assert (= (fiber/resume f) :done) "fiber returning keyword"))

# ============================================================================
# Fiber returning heap value — shared alloc needed, value survives
# ============================================================================

# Return a string (heap)
(let [[f (fiber/new |:yield| (fn [] "hello world"))]]
  (let [[result (fiber/resume f)]]
    (assert (= result "hello world") "fiber returning string")))

# Return a list (heap)
(let [[f (fiber/new |:yield| (fn [] '(1 2 3)))]]
  (let [[result (fiber/resume f)]]
    (assert (= (length result) 3) "fiber returning list length")
    (assert (= (first result) 1) "fiber returning list first")))

# Return an array (heap)
(let [[f (fiber/new |:yield| (fn [] [10 20 30]))]]
  (let [[result (fiber/resume f)]]
    (assert (= (length result) 3) "fiber returning array length")
    (assert (= (get result 0) 10) "fiber returning array element")))

# Return a struct (heap)
(let [[f (fiber/new |:yield| (fn [] {:a 1 :b 2}))]]
  (let [[result (fiber/resume f)]]
    (assert (= (get result :a) 1) "fiber returning struct field")))

# ============================================================================
# Fiber with yield — shared alloc needed for yielded values
# ============================================================================

# Yield a string, return int
(let [[f (fiber/new |:yield| (fn [] (emit 2 "yielded") 99))]]
  (let [[y (fiber/resume f)]]
    (assert (= y "yielded") "fiber yield string")
    (assert (= (fiber/resume f) 99) "fiber return after yield")))

# Yield an array
(let [[f (fiber/new |:yield| (fn [] (emit 2 [1 2 3]) :done))]]
  (let [[y (fiber/resume f)]]
    (assert (= (length y) 3) "fiber yield array length")
    (assert (= (get y 0) 1) "fiber yield array element")))

# ============================================================================
# Fiber with outward mutation of heap value
# ============================================================================

# Captured mutable variable set to heap value inside fiber
(var box nil)
(let [[f (fiber/new |:yield| (fn [] (assign box "escaped") 42))]]
  (fiber/resume f)
  (assert (= box "escaped") "fiber outward mutation string"))

# Captured mutable variable set to array inside fiber
(var holder nil)
(let [[f (fiber/new |:yield| (fn [] (assign holder [1 2 3]) :ok))]]
  (fiber/resume f)
  (assert (= (length holder) 3) "fiber outward mutation array length")
  (assert (= (get holder 1) 2) "fiber outward mutation array element"))

# ============================================================================
# Non-literal lambda passed to fiber/new
# ============================================================================

# defn at top-level, passed as variable
(defn make-string [] "from-defn")

(let [[f (fiber/new |:yield| make-string)]]
  (let [[result (fiber/resume f)]]
    (assert (= result "from-defn") "fiber with defn lambda")))

# Lambda stored in a variable
(let* [[thunk (fn [] {:key "value"})]
       [f (fiber/new |:yield| thunk)]]
  (let [[result (fiber/resume f)]]
    (assert (= (get result :key) "value") "fiber with variable lambda")))

# ============================================================================
# Long-lived reference to fiber return value after handle dropped
# ============================================================================

(def escaped-value
  (let [[f (fiber/new |:yield| (fn [] "long-lived"))]]
    (fiber/resume f)))

# The fiber handle `f` is out of scope; the returned string must survive
(assert (= escaped-value "long-lived") "value survives fiber handle drop")

# Same for a complex value
(def escaped-struct
  (let [[f (fiber/new |:yield| (fn [] {:x 1 :y [2 3]}))]]
    (fiber/resume f)))

(assert (= (get escaped-struct :x) 1) "escaped struct field")
(assert (= (get (get escaped-struct :y) 0) 2) "escaped struct nested array")

# ============================================================================
# Multiple resumes with heap values
# ============================================================================

(let [[f (fiber/new |:yield| (fn []
                      (emit 2 "first")
                      (emit 2 "second")
                      "third"))]]
  (assert (= (fiber/resume f) "first") "multi-yield first")
  (assert (= (fiber/resume f) "second") "multi-yield second")
  (assert (= (fiber/resume f) "third") "multi-yield final"))
