(elle/epoch 10)
# ── box: mutable cell tests ──────────────────────────────────────────
#
# Comprehensive tests for box/unbox/rebox/box? primitives.
# Boxes are mutable cells (LBox) — a single-slot mutable container.

# ── Construction and type ────────────────────────────────────────────

(assert (box? (box 1)) "box creates a box")
(assert (not (box? 1)) "integer is not a box")
(assert (not (box? nil)) "nil is not a box")
(assert (not (box? "hello")) "string is not a box")
(assert (not (box? (list 1))) "list is not a box")

(assert (= (type-of (box 1)) :box) "type-of box is :box")
(assert (= (type-of (box "hi")) :box) "type-of box containing string is :box")
(assert (= (type-of (box nil)) :box) "type-of box containing nil is :box")

# ── type-of through binding ──────────────────────────────────────────
# Box identity must survive binding — (def x (box 3)) must NOT unwrap.

(def x-box (box 3))
(assert (box? x-box) "box? through def binding")
(assert (= (type-of x-box) :box) "type-of box through def binding is :box")

(let [y (box 42)]
  (assert (box? y) "box? through let binding")
  (assert (= (type-of y) :box) "type-of box through let binding is :box"))

(def @m-box (box 99))
(assert (box? m-box) "box? through mutable def binding")
(assert (= (type-of m-box) :box) "type-of box through mutable def binding")

# ── unbox ────────────────────────────────────────────────────────────

(assert (= (unbox (box 42)) 42) "unbox integer")
(assert (= (unbox (box "hello")) "hello") "unbox string")
(assert (= (unbox (box nil)) nil) "unbox nil")
(assert (= (unbox (box true)) true) "unbox bool")
(assert (= (unbox (box :kw)) :kw) "unbox keyword")
(assert (= (unbox (box (list 1 2))) (list 1 2)) "unbox list")
(assert (= (unbox (box [1 2 3])) [1 2 3]) "unbox array")

# unbox through binding
(let [b (box 100)]
  (assert (= (unbox b) 100) "unbox through let binding"))

# ── rebox ────────────────────────────────────────────────────────────

(assert (= (rebox (box 1) 42) 42) "rebox returns new value")

(let [@b (box 1)]
  (rebox b 2)
  (assert (= (unbox b) 2) "rebox updates cell"))

(let [@b (box 1)]
  (rebox b "hello")
  (assert (= (unbox b) "hello") "rebox with different type"))

(let [@b (box 1)]
  (rebox b nil)
  (assert (= (unbox b) nil) "rebox with nil"))

# multiple rebox
(let [@b (box 0)]
  (rebox b 1)
  (rebox b 2)
  (rebox b 3)
  (assert (= (unbox b) 3) "multiple rebox keeps last value"))

# ── box containing box ──────────────────────────────────────────────

(let [inner (box 10)
      outer (box inner)]
  (assert (box? (unbox outer)) "box containing box: inner is box")
  (assert (= (unbox (unbox outer)) 10) "box containing box: double unbox"))

# ── box in collections ──────────────────────────────────────────────

(let [b (box 5)
      arr [b]]
  (assert (box? (get arr 0)) "box in array preserves identity")
  (assert (= (unbox (get arr 0)) 5) "unbox from array"))

(let [b (box 7)
      s {:val b}]
  (assert (box? (get s :val)) "box in struct preserves identity")
  (assert (= (unbox (get s :val)) 7) "unbox from struct"))

# ── box as function argument ────────────────────────────────────────

(defn read-box [b]
  (unbox b))
(defn write-box [b v]
  (rebox b v))

(let [@b (box 0)]
  (write-box b 42)
  (assert (= (read-box b) 42) "box passed to function and modified"))

# ── box in closure capture ──────────────────────────────────────────

(let [@b (box 0)]
  (let [inc-box (fn [] (rebox b (+ (unbox b) 1)))]
    (inc-box)
    (inc-box)
    (inc-box)
    (assert (= (unbox b) 3) "box captured by closure, incremented 3x")))

# ── box with fibers ─────────────────────────────────────────────────

(let [counter (box 0)]
  (def @co
    (fiber/new (fn []
                 (rebox counter (+ (unbox counter) 1))
                 (yield (unbox counter))
                 (rebox counter (+ (unbox counter) 1))
                 (yield (unbox counter))) |:yield|))
  (assert (= (fiber/resume co) 1) "box in fiber: first yield")
  (assert (= (fiber/resume co) 2) "box in fiber: second yield")
  (assert (= (unbox counter) 2) "box in fiber: final value"))

# ── box with fibers ─────────────────────────────────────────────────

(let [b (box 0)
      f (fiber/new (fn []
                     (rebox b 42)
                     (unbox b)) 1)]
  (fiber/resume f)
  (assert (= (fiber/value f) 42) "box modified inside fiber")
  (assert (= (unbox b) 42) "box visible after fiber completes"))

# ── error cases ──────────────────────────────────────────────────────

(let [[ok? _] (protect (unbox 42))]
  (assert (not ok?) "unbox non-box signals error"))

(let [[ok? _] (protect (rebox 42 99))]
  (assert (not ok?) "rebox non-box signals error"))

(let [[ok? _] (protect ((fn () (eval '(box)))))]
  (assert (not ok?) "box arity: no args"))

(let [[ok? _] (protect ((fn () (eval '(box 1 2)))))]
  (assert (not ok?) "box arity: too many args"))

(let [[ok? _] (protect ((fn () (eval '(unbox)))))]
  (assert (not ok?) "unbox arity: no args"))

(let [[ok? _] (protect ((fn () (eval '(rebox (box 1))))))]
  (assert (not ok?) "rebox arity: one arg"))

# ── equality and identity ───────────────────────────────────────────

# Box equality is structural (compares contents), not reference identity.
# Two boxes with the same content compare equal under = and identical?.
(let [a (box 1)
      b (box 1)]
  (assert (= a b) "boxes with same content are =")
  (assert (identical? a b) "boxes with same content are identical?"))

# Mutating one box does not affect the other
(let [@a (box 1)
      @b (box 1)]
  (rebox a 99)
  (assert (= (unbox a) 99) "mutated box has new value")
  (assert (= (unbox b) 1) "other box unchanged")
  (assert (not (= a b)) "boxes diverge after mutation"))

# Same box is identical to itself
(let [a (box 1)]
  (assert (identical? a a) "box is identical to itself"))

# ── box? predicate completeness ──────────────────────────────────────

(assert (not (box? 0)) "box?: integer")
(assert (not (box? 0.0)) "box?: float")
(assert (not (box? true)) "box?: bool")
(assert (not (box? false)) "box?: false")
(assert (not (box? :kw)) "box?: keyword")
(assert (not (box? "s")) "box?: string")
(assert (not (box? ())) "box?: empty list")
(assert (not (box? (list 1))) "box?: list")
(assert (not (box? [1])) "box?: array")
(assert (not (box? @[1])) "box?: mutable array")
(assert (not (box? {:a 1})) "box?: struct")
(assert (not (box? @{:a 1})) "box?: mutable struct")
(assert (not (box? (fn [] 1))) "box?: closure")

# ── eval context (simulates REPL separate compilation) ───────────────
# In the REPL, (def x (box 3)) and (type-of x) are compiled separately.
# eval simulates this: eval'd code goes through the same compile path.

(assert (= (eval '(begin
                    (def eval-box (box 77))
                    (type-of eval-box))) :box)
        "type-of box through eval'd def is :box")

(println "all box tests passed")
