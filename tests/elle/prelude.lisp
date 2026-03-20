(elle/epoch 1)
# Tests for prelude macros: when, unless, try/catch, protect, defer, with,
# butlast, hygiene, case, if-let, when-let, while, forever


# Helper: assert that an expression produced an error via protect
(defn assert-err [result msg]
  "Assert that a protect result indicates failure"
  (assert (= (get result 0) false) msg))

# ============================================================================
# when
# ============================================================================

(assert (= (when true 42) 42) "when true returns body")
(assert (= (when false 42) nil) "when false returns nil")
(assert (= (when true 1 2 3) 3) "when multi-body returns last")
(assert (= (when 1 42) 42) "when truthy non-boolean returns body")

# ============================================================================
# unless
# ============================================================================

(assert (= (unless true 42) nil) "unless true returns nil")
(assert (= (unless false 42) 42) "unless false returns body")
(assert (= (unless false 1 2 3) 3) "unless multi-body returns last")

# ============================================================================
# try/catch
# ============================================================================

(assert (= (try 42 (catch e :error)) 42) "try no error returns body")
(assert (= (try (/ 1 0) (catch e :caught)) :caught) "try catches error")

# try/catch binds the error value
(let ([err-val (try (/ 1 0) (catch e e))])
  (assert (not (nil? err-val)) "try/catch binds error value"))

(assert (= (try 1 2 (+ 20 22) (catch e :error)) 42) "try multi-body returns last")
(assert (= (try (/ 1 0) (catch e 1 2 :caught)) :caught) "try multi-handler returns last")

# Destructured error — kind
(assert (= (try (/ 1 0) (catch {:error kind :message msg} kind)) :division-by-zero) "try destructured error kind")

# Destructured error — message
(assert (= (try (/ 1 0) (catch {:error kind :message msg} msg)) "division by zero") "try destructured error message")

# ============================================================================
# protect
# ============================================================================

# protect success returns [true value]
(let ([result (protect 42)])
  (assert (= (get result 0) true) "protect success flag is true")
  (assert (= (get result 1) 42) "protect success value"))

# protect failure returns [false error]
(let ([result (protect (/ 1 0))])
  (assert (= (get result 0) false) "protect failure flag is false"))

# ============================================================================
# defer
# ============================================================================

# defer runs cleanup
(begin
  (var cleaned false)
  (defer (assign cleaned true) 42)
  (assert cleaned "defer runs cleanup"))

# defer returns body value
(let ([result (begin (var x 0) (defer (assign x 1) 42))])
  (assert (= result 42) "defer returns body value"))

# defer runs cleanup on error
(begin
  (var cleaned false)
  (try (defer (assign cleaned true) (/ 1 0)) (catch e nil))
  (assert cleaned "defer runs cleanup on error"))

# ============================================================================
# with
# ============================================================================

# with basic — returns body value
(let ([result
  (begin
    (defn make-resource [] :resource)
    (defn free-resource [r] nil)
    (with r (make-resource) free-resource
      42))])
  (assert (= result 42) "with returns body value"))

# with cleanup runs
(begin
  (var cleaned false)
  (defn make [] :resource)
  (defn cleanup [r] (assign cleaned true))
  (with r (make) cleanup
    42)
  (assert cleaned "with cleanup runs"))

# ============================================================================
# butlast
# ============================================================================

(assert (= (butlast (list 1 2 3)) (list 1 2)) "butlast basic")
(assert (= (butlast (list 1)) (list)) "butlast single returns empty list")

# butlast on empty list errors
(let ([result (protect (butlast (list)))])
  (let (([ok? _] (protect (result)))) (assert (not ok?) "butlast empty list errors")))

# ============================================================================
# hygiene — prelude macros don't capture user bindings
# ============================================================================

# try macro uses internal binding `f` — user's `f` should not be affected
(assert (= (let ((f 99))
    (try (+ f 1) (catch e :error))) 100) "try hygiene: user binding f not captured")

# defer macro uses internal binding `f` — user's `f` should not be affected
(let ([result
  (begin
    (var cleaned false)
    (let ((f 99))
      (defer (assign cleaned true) (+ f 1))))])
  (assert (= result 100) "defer hygiene: user binding f not captured"))

# ============================================================================
# case — equality dispatch
# ============================================================================

(assert (= (case 2 1 :one 2 :two 3 :three) :two) "case basic match")
(assert (= (case 99 1 :one 2 :two :default) :default) "case default")
(assert (= (case 99 1 :one 2 :two) nil) "case no match no default returns nil")

# case should not double-evaluate the test expression
(begin
  (var counter 0)
  (case (begin (assign counter (+ counter 1)) counter)
    1 :one 2 :two)
  (assert (= counter 1) "case no double eval"))

(assert (= (case "b" "a" 1 "b" 2 "c" 3) 2) "case string keys")
(assert (= (case 1 1 :first 1 :second) :first) "case first match wins")

# ============================================================================
# if-let — conditional binding
# ============================================================================

(assert (= (if-let ((x 42)) x :else) 42) "if-let truthy")
(assert (= (if-let ((x nil)) :then :else) :else) "if-let falsy")
(assert (= (if-let ((x false)) :then :else) :else) "if-let false is falsy")
(assert (= (if-let ((x 1) (y 2)) (+ x y) :else) 3) "if-let multi binding all truthy")
(assert (= (if-let ((x 1) (y nil)) (+ x y) :else) :else) "if-let multi binding second falsy")
(assert (= (if-let ([x 42]) x :else) 42) "if-let bracket binding")
(assert (= (if-let ([x nil]) :then :else) :else) "if-let bracket binding falsy")
(assert (= (if-let ([x 1] [y 2]) (+ x y) :else) 3) "if-let bracket multi binding")

# ============================================================================
# when-let — conditional binding without else
# ============================================================================

(assert (= (when-let ((x 42)) x) 42) "when-let truthy")
(assert (= (when-let ((x nil)) x) nil) "when-let falsy returns nil")
(assert (= (when-let ((x 1)) (+ x 1) (+ x 2)) 3) "when-let multi body returns last")
(assert (= (when-let ([x 42]) (+ x 1)) 43) "when-let bracket binding")

# ============================================================================
# while — multi-body forms
# ============================================================================

# while with multiple body forms
(begin
  (var n 0)
  (var sum 0)
  (while (< n 3)
    (assign sum (+ sum n))
    (assign n (+ n 1)))
  (assert (= sum 3) "while multi body"))

# while with single body
(begin
  (var n 0)
  (while (< n 5) (assign n (+ n 1)))
  (assert (= n 5) "while single body"))

# ============================================================================
# forever — infinite loop with break
# ============================================================================

# forever with break
(begin
  (var n 0)
  (forever
    (assign n (+ n 1))
    (if (= n 5) (break)))
  (assert (= n 5) "forever with break"))

# forever break with value
(let ([result
  (begin
    (var n 0)
    (forever
      (assign n (+ n 1))
      (if (= n 3) (break :while :done))))])
  (assert (= result :done) "forever break value"))
