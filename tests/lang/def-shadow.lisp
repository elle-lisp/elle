(elle/epoch 18)
# audited: 2026-11-19
# A def's value cannot read the name it binds, a function in it reads the new binding, and an explicit letrec rejects a duplicate.
# docs/bindings.md
#
# The counter-factuals: a function inside a redefinition's value read the
# previous binding, so a redefined function called the one it replaced. A
# self-call from a function passed to a wrapper called that function, so it
# skipped the wrapper.
#
# Up to epoch 17 a redefinition's value read the previous binding. A name now
# exists from its def onward, so a direct read of it in its own def's value is
# a compile error, and a function there that runs before the def finds no
# value. A pattern binds its leaves after its value, as let does, so a function
# in a pattern's value reads the leaves before it.

(defn compile-error [src]
  (let [[ok? err] (protect (compile/whole-module src "<def-shadow>"))]
    (assert (not ok?) (string "expected a compile error: " src))
    (get err :message)))

(defn run [src]
  "Compile src as a file and run it, answering what protect answers."
  (protect ((get (get (compile/whole-module src "<def-shadow>") 0) 1))))

(defn counter []
  (def @calls 0)
  (defn counted [f]
    (fn [n]
      (assign calls (+ calls 1))
      (f n)))
  [counted (fn [] calls)])

# ── file scope: the value cannot read the name it binds ──
(assert (string/contains? (compile-error "(def a 10) (def a (+ a 1)) a")
                          "'a' referenced before its initialization")
        "file-scope: a redefinition's value cannot read its own name")
(assert (string/contains? (compile-error
                            "(def @x @[1 2 3]) (def @x (freeze x)) x")
                          "'x' referenced before its initialization")
        "file-scope: nor can a mutable one's")

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

(let [[ok? _] (run "(def early 1) (def early ((fn [] (+ early 1)))) early")]
  (assert (not ok?)
          "file-scope: a function in a redefinition's value, run before the def, finds no value"))

(def [pa pb] [1 2])
(def [pa pb] [(fn [] pb) 5])
(assert (= (pa) 2)
        "file-scope: a function in a pattern's value reads the leaf before it")
(assert (= pb 5) "file-scope: and the form after it reads the new leaf")

# ── function body: the same rules ──
(defn body-redefinitions []
  (def [counted calls] (counter))
  (defn climb [n]
    :replaced)
  (defn climb [n]
    (if (> n 30) n (climb (+ n 10))))
  (def wrapped (fn [n] :replaced))
  (def wrapped (counted (fn [n] (if (> n 30) n (wrapped (+ n 10))))))
  (def [pa pb] [1 2])
  (def [pa pb] [(fn [] pb) 5])
  [(climb 1) (wrapped 1) (calls) (pa) pb])
(assert (= (body-redefinitions) [31 31 4 2 5])
        "body: a redefined function calls itself, through a wrapper too")

(defn body-first-wrapped []
  (def [counted calls] (counter))
  (def go (counted (fn [n] (if (> n 0) (go (- n 1)) :done))))
  [(go 3) (calls)])
(assert (= (body-first-wrapped) [:done 4])
        "body: a first def's self-call runs the wrapper")

(defn body-early []
  (def early 1)
  (def early ((fn [] (+ early 1))))
  early)
(let [[ok? _] (protect (body-early))]
  (assert (not ok?)
          "body: a function in a redefinition's value, run before the def, finds no value"))

(defn body-first-early []
  (def level 1)
  (defn deeper []
    (def level ((fn [] (+ level 1))))
    level)
  (deeper))
(let [[ok? _] (protect (body-first-early))]
  (assert (not ok?)
          "body: a function in a first def's value, run before the def, finds no value"))

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
