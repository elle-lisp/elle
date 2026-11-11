(elle/epoch 15)
# audited: 2026-11-09
# A letrec binding a sibling closure captures costs one env cell per activation, and that cell is released.
# docs/impl/region/cells.md
#
# Inside a function, a `letrec` binding a sibling captures lives in the
# activation's environment unless its init is a closure: `populate_env` mints
# one `CaptureCell` for it, in a region of its own, once per call. The binding
# is prebound, so it takes the cell even when it is immutable. Nothing frees the
# cell unless the walk records a release placeholder for it, as it does for a
# `let` or a `def`, and the cell holds its content alive.
#
# The counter-factual: the walk's `Letrec` arm and `lower_letrec` recorded no
# placeholder, so every subject leaked the cell per call, and the array
# subjects leaked the array the cell holds as well.
#
# Each drive runs the same call `calls` times after an identical warmup, so a
# per-call cost reads `calls` or a multiple, and a one-off reads about 0. The
# control fixes the window's own bookkeeping cost, which `slack` is sized for.

(def calls 200)
(def slack 20)

# ── control: a `let` binding a closure captures, released today ──────

(defn let-capture []
  (let [v @[1 2 3]
        g (fn [] (first v))]
    (g)))

# ── subjects: one env cell per captured letrec binding ───────────────

(defn letrec-immediate []
  (letrec [v 5
           g (fn [] v)]
    (g)))

(defn letrec-array []
  (letrec [v @[1 2 3]
           g (fn [] (first v))]
    (g)))

# The closure reads the binding before it is defined in the letrec.
(defn letrec-forward []
  (letrec [g (fn [] (first v))
           v @[1 2 3]]
    (g)))

(defn letrec-mutable []
  (letrec [@v @[1]
           g (fn []
               (assign v @[2])
               (first v))]
    (g)))

# The letrec runs on every pass of a loop, and the activation mints the cell
# once, so its release runs once, after the loop.
(defn letrec-in-loop []
  (def @i 0)
  (while (< i 3)
    (letrec [v @[i]
             g (fn [] (first v))]
      (g))
    (assign i (+ i 1)))
  i)

(defmacro drive [label form]
  `(begin
     (def @w0 0)
     (while (< w0 calls)
       (begin
         ,form)
       (assign w0 (+ w0 1)))
     (def c0 (arena/count))
     (def r0 (arena/region-count))
     (def @w1 0)
     (while (< w1 calls)
       (begin
         ,form)
       (assign w1 (+ w1 1)))
     (def objects (- (arena/count) c0))
     (def regions (- (arena/region-count) r0))
     (println "  " ,label " obj=" objects " reg=" regions)
     (assert (<= objects slack)
             (string ,label " leaks objects per call, delta=" objects))
     (assert (<= regions slack)
             (string ,label " leaks regions per call, delta=" regions))))

(println "region-letrec-capture-cell-leak over " calls " calls:")

(drive "let-capture     " (let-capture))
(drive "letrec-immediate" (letrec-immediate))
(drive "letrec-array    " (letrec-array))
(drive "letrec-forward  " (letrec-forward))
(drive "letrec-mutable  " (letrec-mutable))
(drive "letrec-in-loop  " (letrec-in-loop))

# ── the cell outlives the call that made it ──────────────────────────
#
# A closure the function returns still holds the cell, and reads through it
# after the call has released the activation's own reference. Churn between the
# call and the read recycles a page the release freed too early, which the
# sidecar's guardfree turns into a fault.

(defn letrec-returned []
  (letrec [v @[1 2 3]
           g (fn [] (first v))]
    g))

(defn churn []
  (def @k 0)
  (while (< k 64)
    (def junk @[k k k])
    (assign k (+ k 1))))

(def reader (letrec-returned))
(churn)
(assert (= (reader) 1)
        "a returned closure reads its letrec sibling after the call")
(assert (= (letrec-array) 1) "the closure reads its letrec sibling")
(assert (= (letrec-forward) 1) "the closure reads a sibling bound after it")
(assert (= (letrec-mutable) 2) "the closure reads what it assigned")
(assert (= (letrec-in-loop) 3) "the loop runs every pass")

(println "region-letrec-capture-cell-leak: ok")
