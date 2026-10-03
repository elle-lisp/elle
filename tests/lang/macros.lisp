(elle/epoch 14)
# audited: 2026-09-30
# A macro expands where it is called, may expand into another macro, and calls helpers defined by begin-for-syntax.
# docs/macros.md

# ── Expansion at the call site ───────────────────────────────────────

# A closure in a template captures the variable the caller passed in.
(defmacro make-adder [n]
  `(fn [x] (+ x ,n)))

(let [amount 5
      add-amount (make-adder amount)]
  (assert (= (add-amount 10) 15)
          "a template closure captures a call-site variable"))

# The trap: a false argument wrapped as a syntax object is truthy, so this
# macro took its body on a false test.
(defmacro when-true [test body]
  `(if ,test ,body nil))

(assert (nil? (when-true false 42)) "a false argument stays false")
(assert (= (when-true true 42) 42) "a true argument stays true")

# A macro may expand into a call of another macro. The arguments keep their
# call-site bindings through both expansions.
(defmacro inner-add [x y]
  `(+ ,x ,y))
(defmacro outer-add [a b]
  `(inner-add ,a ,b))

(let [x 10
      y 20]
  (assert (= (outer-add x y) 30) "arguments survive two expansions"))

# ── gensym ───────────────────────────────────────────────────────────

# The counter-factual: a gensym that answered a string would put a string
# where the template binds a name, and the expansion would not compile.
(defmacro with-temp [body]
  (let [tmp (gensym "tmp")]
    `(let [,tmp 42]
       ,body)))

(assert (= (with-temp (+ 1 2)) 3) "a template binds the symbol gensym makes")
(assert (symbol? (gensym "v")) "gensym answers a symbol")
(assert (not (= (gensym "v") (gensym "v"))) "each gensym is a fresh symbol")

# ── begin-for-syntax ─────────────────────────────────────────────────

# A definition inside begin-for-syntax exists when macros expand, so a macro
# body can call it.
(begin-for-syntax (def add-one (fn [x] (%add x 1))))

(defmacro inc-literal [n]
  (add-one (syntax->datum n)))

(assert (= (inc-literal 5) 6) "a macro calls a begin-for-syntax helper")
(assert (= (inc-literal 41) 42) "the helper runs at each expansion")

# A later begin-for-syntax block sees the definitions of an earlier one.
(begin-for-syntax (def double (fn [x] (%mul x 2))))

(begin-for-syntax (def quad (fn [x] (double (double x)))))

(defmacro quadruple [n]
  (quad (syntax->datum n)))

(assert (= (quadruple 3) 12) "a second begin-for-syntax block calls the first")
