(elle/epoch 12)
# audited: 2026-09-22
# An eval's result, and a value a returning position hands out, each carry one
# reference: a loop that evaluates stays flat in regions and objects.
#
# src/hir/anf.rs (§ "A returning position names only what it must release")
#
# Two shapes owe a release that only a slot can route, and each is measured
# here at every position that reaches it:
#
#   * the ROOT of an eval'd unit is a returning position where no call is a tail
#     call, so `(eval '(pair 1 2))` compiles a call whose owned result then takes
#     the `Return` mint as well;
#   * an `eval` at a FUNCTION TAIL is never a tail call, so `(fn () (eval x))`
#     returns the eval's owned result with the mint on top of it.
#
# A `parameterize` body is the third route to the same shape: it is never a
# tail position, so its call's owned result leaves through the enclosing
# function's `Return` mint.
#
# Each leak here is one whole region per iteration, so a surviving defect reads
# ~2000 over the window. The control is an eval whose result is an immediate,
# which has no region to strand.
#
# The soundness complement is the value checks at the bottom: a named result is
# released by its slot, so a release placed one step too early frees a value the
# caller then reads. Run this file under `--trace=guardfree` to make that read
# fault rather than read back a recycled page.

(def window 2000)

(defn measure [thunk]
  (var i 0)
  (while (< i 200)
    (thunk)
    (assign i (+ i 1)))
  (def regions (arena/region-count))
  (def objects (arena/count))
  (var j 0)
  (while (< j window)
    (thunk)
    (assign j (+ j 1)))
  [(- (arena/region-count) regions) (- (arena/count) objects)])

(defn bounded? [row label]
  (def [regions objects] row)
  (assert (< regions 100) (string label " strands regions, delta=" regions))
  (assert (< objects 100) (string label " strands objects, delta=" objects)))

# ── the root of the eval'd unit ───────────────────────────────────────────────
# Statement position: the driver discards the result, so any growth is the
# eval's own.
(def imm-d
  (measure (fn ()
             (eval 7)
             nil)))
(def call-d
  (measure (fn ()
             (eval '(pair 1 2))
             nil)))
(def array-d
  (measure (fn ()
             (eval '[1 2])
             nil)))
(def let-call-d
  (measure (fn ()
             (eval '(let [a 1]
                      (pair a 2)))
             nil)))
(def bound-d
  (measure (fn ()
             (let [x (eval '(pair 1 2))]
               (first x)))))

# ── an eval at a function tail ────────────────────────────────────────────────
# The driver returns the eval's result, so it takes the driver's `Return` mint.
# A string literal and a closure allocate fresh in the eval'd unit, which is
# balanced by itself: what is measured is the driver's frame.
(def tail-str-d (measure (fn () (eval "hello"))))
(def tail-fn-d (measure (fn () (eval '(fn (x) x)))))
(def tail-call-d (measure (fn () (eval '(pair 1 2)))))
(def tail-let-d
  (measure (fn ()
             (let [form '(pair 1 2)]
               (eval form)))))

# ── a parameterize body at a function tail ────────────────────────────────────
(defn in-param []
  (parameterize ((*spawn* nil))
    (pair 1 2)))
(def param-d
  (measure (fn ()
             (in-param)
             nil)))

# ── inline, in the driver's own loop ──────────────────────────────────────────
# No thunk between the loop and the eval: the issue's own reproduction.
(def inline-before (arena/region-count))
(def @k 0)
(while (< k window)
  (eval '(pair 1 2))
  (assign k (+ k 1)))
(def inline-d (- (arena/region-count) inline-before))

(println "region-eval-return-leak deltas over " window
         " iterations, [regions objects]:")
(println "  root   : imm " imm-d "  call " call-d "  array " array-d
         "  let-call " let-call-d "  bound " bound-d)
(println "  tail   : str " tail-str-d "  fn " tail-fn-d "  call " tail-call-d
         "  let " tail-let-d)
(println "  param  : " param-d "   inline regions: " inline-d)

(bounded? imm-d "control: eval of an immediate")
(bounded? call-d "a call at the eval'd root")
(bounded? array-d "an array literal at the eval'd root")
(bounded? let-call-d "a call in a let body at the eval'd root")
(bounded? bound-d "a let-bound eval of a call")
(bounded? tail-str-d "an eval at a function tail (string)")
(bounded? tail-fn-d "an eval at a function tail (closure)")
(bounded? tail-call-d "an eval at a function tail (call)")
(bounded? tail-let-d "an eval in a let body at a function tail")
(bounded? param-d "a call in a parameterize body at a function tail")
(assert (< inline-d 100)
        (string "an eval inline in a loop strands regions, delta=" inline-d))

# ── the values survive their release ──────────────────────────────────────────
(defn eval-at-tail []
  (eval '(pair 1 2)))
(defn eval-str-at-tail []
  (eval "hello"))
(assert (= (eval '(pair 1 2)) (pair 1 2)) "a root call's result")
(assert (= (eval '[1 2]) [1 2]) "a root array literal's result")
(assert (= (eval '(let [a 1]
                    (pair a 2))) (pair 1 2)) "a root let body's result")
(assert (= (first (eval-at-tail)) 1) "an eval result returned from a function")
(assert (= (eval-str-at-tail) "hello")
        "an eval'd string returned from a function")
(assert (= (rest (in-param)) 2) "a parameterize body's result")
(def @kept @[])
(var n 0)
(while (< n 50)
  (push kept (eval-at-tail))
  (assign n (+ n 1)))
(each p in kept
  (assert (= p (pair 1 2)) "a kept eval result reads back intact"))

(println "region-eval-return-leak: ok")
