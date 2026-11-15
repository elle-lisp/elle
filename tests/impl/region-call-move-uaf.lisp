(elle/epoch 15)
# audited: 2026-11-13
# A moved argument stays live for every holder the callee gives it, and a call never moves a value it still reads.
# docs/impl/region/handoff.md
#
# The soundness complement of region-call-traffic.lisp. The sidecar arms
# guardfree, so a released page is unmapped and a stale read faults at its
# deref, and compiles on the VM thread, so each loop runs its first ten
# iterations in the interpreter and the rest compiled.
#
# A call that is an argument's last use offers the frame's reference to a
# closure callee, and the frame's slot reads `nil` once the callee takes it.
# The callee's release of its parameter then answers for the value alone.
#
# Shapes 1, 2, 3 and 8 take the move, and give the value another holder or
# leave the frame by a signal before the callee's release runs. Shapes 4, 5,
# 6 and 7 are the ones the lowerer must refuse: in each, a second name for the
# value is still read after the callee's release of the first would have freed
# it, had the call moved the frame's reference. In shape 7 that name is the
# callee's own capture, and the capture's adopt pins the region.
#
# Each call is a `let` init, never a tail call, which moves its arguments by
# another rule (docs/impl/region/rules.md). Every read below happens after
# the callee's release of its parameter, and each iteration builds a fresh
# string, so a recycled region detonates on its generation stamp.

(def over-frees (arena/over-frees))
(def rounds 40)

# ── 1. the callee stores what it was handed ──────────────────────────────────

(def @sink @[])
(defn keep [g]
  (push sink g)
  nil)
(defn store-it [n]
  (keep (string "kept-" n))
  0)

(defn wrap [g]
  (fn [] g))
(def @closures @[])
(defn close-over-it [n]
  (let [c (wrap (string "closed-" n))]
    (push closures c)
    0))

(var i 0)
(while (< i rounds)
  (store-it i)
  (close-over-it i)
  (assign i (inc i)))
(assign i 0)
(while (< i rounds)
  (assert (= (sink i) (string "kept-" i)) "a stored argument survives its move")
  (assert (= ((closures i)) (string "closed-" i))
          "a captured argument survives its move")
  (assign i (inc i)))

# ── 2. the callee raises after it took the move ──────────────────────────────
# The raise parks the caller's frame, and the catch abandons it. The walk over
# the abandoned frame finds `nil` in the moved slot, so the release runs once,
# in the callee's error path or nowhere.

(defn boom [g]
  (error {:error :boom :value g}))
(defn raise-it [n]
  (let [t (string "boom-" n)]
    (boom t)
    0))

(assign i 0)
(while (< i rounds)
  (let [[ok? e] (protect (raise-it i))]
    (assert (not ok?) "the callee raised")
    (assert (= (e :value) (string "boom-" i)) "the raised payload survives"))
  (assign i (inc i)))

# ── 3. the callee yields: resumed to its end, and abandoned ──────────────────

(defn pause [g]
  (yield g)
  (length g))
(defn yield-it [n]
  (let [t (string "yield-" n)]
    (pause t)
    0))

(assign i 0)
(while (< i rounds)
  (let [f (fiber/new (fn [] (yield-it i)) |:yield|)]
    (assert (= (fiber/resume f) (string "yield-" i)) "the yielded argument")
    (assert (= (fiber/resume f) 0) "the resumed call ends"))
  (let [f (fiber/new (fn [] (yield-it i)) |:yield|)
        v (fiber/resume f)]
    (assert (= v (string "yield-" i))
            "the yielded argument of an abandoned call"))
  (assign i (inc i)))

# ── 4. a closure passed to itself ────────────────────────────────────────────
# The callee releases its parameter after the first read, then reads its own
# environment. Moved, that release frees the closure it is running.

(defn mk-self [n]
  (let [tag (string "self-" n)]
    (fn [f]
      (def same? (identical? f f))
      [same? (length tag)])))
(defn self-it [n]
  (let [h (mk-self n)
        r (h h)]
    r))

(assign i 0)
(while (< i rounds)
  (let [[same? len] (self-it i)]
    (assert same? "the closure saw itself")
    (assert (= len (length (string "self-" i))) "the closure read its own env"))
  (assign i (inc i)))

# ── 5. one value in two positions ────────────────────────────────────────────

(defn both [a b]
  (def x (length a))
  (+ x (length b)))
(defn twice-it [n]
  (let [t (string "twice-" n)
        r (both t t)]
    r))

(assign i 0)
(while (< i rounds)
  (assert (= (twice-it i) (* 2 (length (string "twice-" i))))
          "both positions read the one value")
  (assign i (inc i)))

# ── 6. a value and a borrow out of it ────────────────────────────────────────
# `(first t)` lives in `t`'s list. The callee releases `a` before it reads `b`.

(defn pair-read [a b]
  (def x (length a))
  (+ x (length b)))
(defn borrow-it [n]
  (let [t (list (string "inner-" n) 1)
        r (pair-read t (first t))]
    r))

(assign i 0)
(while (< i rounds)
  (assert (= (borrow-it i) (+ 2 (length (string "inner-" i))))
          "the borrow outlives the callee's release of its container")
  (assign i (inc i)))

# ── 7. the callee closes over a value the frame passes it ───────────────────
# `h` captures `t`, and the frame passes `t` to `h` as well. The callee's
# release of its parameter must leave the capture standing.

(defn capture-it [n]
  (let [t (string "cap-" n)
        h (fn [g]
            (def x (length g))
            (+ x (length t)))
        r (h t)]
    r))

(assign i 0)
(while (< i rounds)
  (assert (= (capture-it i) (* 2 (length (string "cap-" i))))
          "the capture outlives the callee's release of the argument")
  (assign i (inc i)))

# ── 8. two arguments named in place ──────────────────────────────────────────
# ANF names each argument inside its own position, and the call moves both.
# The callee releases the first before it stores the second.

(def @seconds @[])
(defn keep-second [a b]
  (def x (length a))
  (push seconds b)
  x)
(defn both-in-place [n]
  (keep-second (string "first-" n) (string "second-" n))
  0)

(assign i 0)
(while (< i rounds)
  (both-in-place i)
  (assign i (inc i)))
(assign i 0)
(while (< i rounds)
  (assert (= (seconds i) (string "second-" i))
          "the second moved argument survives the release of the first")
  (assign i (inc i)))

(assert (= (arena/over-frees) over-frees) "no release ran past zero")

(println "region-call-move-uaf: ok")
