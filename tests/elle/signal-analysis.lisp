# ── Signal analysis tests ─────────────────────────────────────────────
#
# Lock down expectations for compute_inferred_signal: which functions
# have which signal bits, and how signals compose across calls.

# ── fn/errors?: error-only functions ─────────────────────────────────

(assert (fn/errors? (fn [x] (error "boom")))
  "explicit error produces SIG_ERROR")

(assert (fn/errors? (fn [a b] (+ a b)))
  "arithmetic has SIG_ERROR (type checking)")

(assert (fn/errors? (fn [a b] (* a b)))
  "multiplication has SIG_ERROR")

(assert (fn/errors? (fn [a b] (/ a b)))
  "division has SIG_ERROR")

(assert (fn/errors? (fn [a b] (- a b)))
  "subtraction has SIG_ERROR")

(assert (fn/errors? (fn [a b] (< a b)))
  "comparison has SIG_ERROR")

(assert (not (fn/errors? (fn [x] x)))
  "identity has no SIG_ERROR")

(assert (not (fn/errors? (fn [] 42)))
  "constant has no SIG_ERROR")

(assert (not (fn/errors? (fn [x] (if x 1 0))))
  "if with constants has no SIG_ERROR")

# ── silent?: suspension check ────────────────────────────────────────

(assert (silent? (fn [x] x))
  "identity is silent")

(assert (silent? (fn [a b] (+ a b)))
  "arithmetic is silent (error doesn't suspend)")

(assert (silent? (fn [x] (error "boom")))
  "error-only is silent (error doesn't suspend)")

(assert (not (silent? (fn [x] (yield x))))
  "yield is not silent")

(assert (not (silent? (fn [x] (println x))))
  "I/O is not silent")

# ── Error propagation through calls ──────────────────────────────────

(defn inner-error [x] (error "boom"))
(defn calls-error [x] (inner-error x))
(assert (fn/errors? calls-error)
  "calling an error function propagates SIG_ERROR")

(defn inner-pure [x] x)
(defn calls-pure [x] (inner-pure x))
(assert (not (fn/errors? calls-pure))
  "calling a pure function does not add SIG_ERROR")

(defn arith-wrapper [a b] (+ a b))
(defn double-wrap [x] (arith-wrapper x x))
(assert (fn/errors? double-wrap)
  "SIG_ERROR propagates through chains of arithmetic wrappers")

# ── Yield propagation ────────────────────────────────────────────────

(defn yielder [x] (yield x))
(defn calls-yielder [x] (yielder x))
(assert (not (silent? calls-yielder))
  "calling a yielding function propagates suspension")

(assert (silent? calls-pure)
  "calling a pure function stays silent")

# ── Mixed signals ────────────────────────────────────────────────────

(defn error-and-yield [x]
  (if (> x 0) (yield x) (error "negative")))
(assert (fn/errors? error-and-yield)
  "mixed function has SIG_ERROR")
(assert (not (silent? error-and-yield))
  "mixed function is not silent (has yield)")

# ── Conditional paths ────────────────────────────────────────────────

(defn maybe-error [x]
  (if x (error "boom") 42))
(assert (fn/errors? maybe-error)
  "error in one branch produces SIG_ERROR")

(defn maybe-yield [x]
  (if x (yield 1) 42))
(assert (not (silent? maybe-yield))
  "yield in one branch produces suspension")

(defn pure-branches [x]
  (if x 1 0))
(assert (not (fn/errors? pure-branches))
  "pure branches have no SIG_ERROR")
(assert (silent? pure-branches)
  "pure branches are silent")

# ── Closures with captures ───────────────────────────────────────────

(def outer-val 10)
(def capturing (fn [x] (+ x outer-val)))
(assert (fn/errors? capturing)
  "capturing closure with arithmetic has SIG_ERROR")
(assert (silent? capturing)
  "capturing closure with arithmetic is silent")

# ── compile/signal API ───────────────────────────────────────────────

(def a (compile/analyze "(defn f [x] x)"))
(def sig (compile/signal a :f))
(assert (get sig :silent) "identity is silent in compile/signal")
(assert (not (get sig :yields)) "identity doesn't yield")
(assert (not (get sig :io)) "identity has no I/O")
(assert (empty? (get sig :bits)) "identity has empty bits")

(def a2 (compile/analyze "(defn g [x] (+ x 1))"))
(def sig2 (compile/signal a2 :g))
(assert (not (get sig2 :silent)) "arithmetic not silent in compile/signal")
(assert (not (get sig2 :yields)) "arithmetic doesn't yield")
(assert (has? (get sig2 :bits) :error) "arithmetic has :error in bits")
(assert (get sig2 :jit-eligible) "arithmetic is jit-eligible")

(def a3 (compile/analyze "(defn h [x] (println x))"))
(def sig3 (compile/signal a3 :h))
(assert (not (get sig3 :silent)) "I/O not silent")
(assert (get sig3 :yields) "I/O yields")
(assert (not (get sig3 :jit-eligible)) "I/O not jit-eligible")

# ── compile/query-signal ─────────────────────────────────────────────

(def a4 (compile/analyze "
  (defn pure [x] x)
  (defn arith [x] (+ x 1))
  (defn io-fn [x] (println x))
"))
(def silent-fns (compile/query-signal a4 :silent))
(def yielding-fns (compile/query-signal a4 :yields))
(def jit-fns (compile/query-signal a4 :jit-eligible))

# pure is silent; arith has SIG_ERROR so not silent; io-fn suspends
(assert (= (length silent-fns) 1) "one silent function")
(assert (= (get (first silent-fns) :name) "pure") "pure is the silent one")

# io-fn yields; pure and arith don't
(assert (= (length yielding-fns) 1) "one yielding function")
(assert (= (get (first yielding-fns) :name) "io-fn") "io-fn is the yielding one")

# pure and arith are jit-eligible; io-fn is not
(assert (= (length jit-fns) 2) "two jit-eligible functions")

(println "all signal analysis tests passed")
