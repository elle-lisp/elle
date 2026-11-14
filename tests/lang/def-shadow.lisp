(elle/epoch 16)
# audited: 2026-11-14
# A redefinition's value reads the previous binding, a function in it reads the new one, and an explicit letrec rejects a duplicate.
# docs/bindings.md
#
# The counter-factuals: the file registered a redefinition before analyzing its
# value, so `(def a (+ a 1))` read the new, uninitialized `a`. A function inside
# a redefinition's value read the previous binding, so a redefined function
# called the one it replaced. A self-call from a function passed to a wrapper
# called that function, so it skipped the wrapper.

(defn compile-error [src]
  (let [[ok? err] (protect (compile/whole-module src "<def-shadow>"))]
    (assert (not ok?) (string "expected a compile error: " src))
    (get err :message)))

(defn counter []
  (def @calls 0)
  (defn counted [f]
    (fn [n]
      (assign calls (+ calls 1))
      (f n)))
  [counted (fn [] calls)])

# ── file scope: the redefinition's value reads the previous binding ──
(def a 10)
(def a (+ a 1))
(assert (= a 11) "file-scope: (+ a 1) should see previous a=10")

(def @x @[1 2 3])
(def @x (freeze x))
(assert (= x [1 2 3]) "file-scope: freeze of @x should produce [1 2 3]")

# ── file scope: a function in the value reads the new binding ──
(defn climb [n]
  :replaced)
(defn climb [n]
  (if (> n 30) n (climb (+ n 10))))
(assert (= (climb 1) 31) "file-scope: a redefined defn calls itself")

(def [file-counted file-calls] (counter))
(def wrapped-climb (fn [n] :replaced))
(def wrapped-climb
  (file-counted (fn [n] (if (> n 30) n (wrapped-climb (+ n 10))))))
(assert (= (wrapped-climb 1) 31) "file-scope: a wrapped redefinition recurses")
(assert (= (file-calls) 4) "file-scope: each recursive call runs the wrapper")

(def [first-counted first-calls] (counter))
(def first-wrapped
  (first-counted (fn [n] (if (> n 0) (first-wrapped (- n 1)) :done))))
(assert (= (first-wrapped 3) :done) "file-scope: a wrapped first def recurses")
(assert (= (first-calls) 4)
        "file-scope: a first def's self-call runs the wrapper")

(def early 1)
(def early ((fn [] (+ early 1))))
(assert (= early 2)
        "file-scope: before it runs, a redefinition holds the previous value")

(def [pa pb] [1 2])
(def [pa pb] [(fn [] pb) 5])
(assert (= (pa) 5)
        "file-scope: a function in a pattern's value reads the new leaf")

# ── function body: the same rules ──
(defn body-redefinitions []
  (def [counted calls] (counter))
  (defn climb [n]
    :replaced)
  (defn climb [n]
    (if (> n 30) n (climb (+ n 10))))
  (def wrapped (fn [n] :replaced))
  (def wrapped (counted (fn [n] (if (> n 30) n (wrapped (+ n 10))))))
  (def early 1)
  (def early ((fn [] (+ early 1))))
  (def [pa pb] [1 2])
  (def [pa pb] [(fn [] pb) 5])
  [(climb 1) (wrapped 1) (calls) early (pa)])
(assert (= (body-redefinitions) [31 31 4 2 5])
        "body: a redefined function calls itself, through a wrapper too")

(defn body-first-wrapped []
  (def [counted calls] (counter))
  (def go (counted (fn [n] (if (> n 0) (go (- n 1)) :done))))
  [(go 3) (calls)])
(assert (= (body-first-wrapped) [:done 4])
        "body: a first def's self-call runs the wrapper")

(defn body-first-early []
  (def level 1)
  (defn deeper []
    (def level ((fn [] (+ level 1))))
    level)
  (deeper))
(assert (= (body-first-early) 2)
        "body: a function in a first def's value, called before it, reads the outer binding")

# ── explicit letrec: a self-call through a wrapper runs the wrapper ──
(let [[counted calls] (counter)]
  (letrec [go (counted (fn [n] (if (> n 0) (go (- n 1)) :done)))]
    (assert (= (go 3) :done) "letrec: a wrapped binding recurses")
    (assert (= (calls) 4) "letrec: each recursive call runs the wrapper")))

# ── file scope: defs of one name agree on mutability ──
(assert (string/contains? (compile-error "(def @x @[1 2 3]) (def x (freeze x))")
                          "both mutable and immutable")
        "file-scope: @x then x is a compile error")

# ── explicit letrec: duplicate names rejected ──
(let [[ok? _] (protect ((fn []
                          (eval '(letrec [x 1
                                          x 2]
                                   x)))))]
  (assert (not ok?) "letrec: duplicate x should be a compile error"))

(let [[ok? _] (protect ((fn []
                          (eval '(letrec [@x @[1 2 3]
                                          x (freeze x)]
                                   x)))))]
  (assert (not ok?) "letrec: @x then x should be a compile error"))
