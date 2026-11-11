(elle/epoch 15)
# audited: 2026-11-10
# A reassigned captured binding's binder drops its init's reference once, and only where the init allocated.
# docs/impl/region/cells.md
#
# A captured binding that some `assign` repoints never releases its init
# through the cell's slot, because by the release the cell names another
# value. Its binder drops the producer's reference itself, after the store,
# and the cell's own counted reference holds the init from there.
#
# The counter-factuals, one per section:
#
# - The leak: an env cell's binder stored the init and dropped nothing, so a
#   call-result init leaked one region per call under `let`, `def` and
#   `letrec`. A closure init leaked only when the body ended in a tail call,
#   because its release by region id landed after the call.
# - The alias: a compiled cell's binder dropped an init that only names
#   another binding. That init holds no reference of its own, so the drop took
#   the other binding's, and the first `assign` freed the value under it. The
#   sidecar's guardfree turns the read into a fault.
#
# Each drive runs the same call `calls` times after an identical warmup, so a
# per-call cost reads `calls` or a multiple, and a one-off reads about 0.

(def calls 200)
(def slack 20)

# ── controls: no init the binder owes a drop ─────────────────────────

(defn captured-stable []
  (let [@v @[1]
        g (fn [] v)]
    (first (g))))

(defn uncaptured-reassigned []
  (let [@v @[1]]
    (assign v @[2])
    (first v)))

(defn immediate-init []
  (let [@v 1
        g (fn [] (first v))]
    (assign v @[2])
    (g)))

# ── subjects: a call-result init, one row per binder ─────────────────

(defn let-assign-inside []
  (let [@v @[1]
        g (fn []
            (assign v @[2])
            (first v))]
    (g)))

(defn let-assign-outside []
  (let [@v @[1]
        g (fn [] (first v))]
    (assign v @[2])
    (g)))

(defn def-assign-inside []
  (def @v @[1])
  (defn g []
    (assign v @[2])
    (first v))
  (g))

(defn def-assign-outside []
  (def @v @[1])
  (defn g []
    (first v))
  (assign v @[2])
  (g))

(defn letrec-assign-outside []
  (letrec [@v @[1]
           g (fn [] (first v))]
    (assign v @[2])
    (g)))

# A closure init, released by its region id, and a body that ends in a tail
# call the frame does not come back from.
(defn closure-init-tail []
  (let [@v (fn [] 1)
        g (fn [] v)]
    (assign v 2)
    (g)))

# ── an init that names another binding owns no reference ─────────────

(defn alias-param [y]
  (let [@v y
        g (fn [] v)]
    (assign v 2)
    (g))
  (first y))

(defn alias-local []
  (def y @[7 8])
  (let [@v y
        g (fn [] v)]
    (assign v 2)
    (g))
  (first y))

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

(println "region-capture-cell-init-drop over " calls " calls:")

(drive "captured-stable      " (captured-stable))
(drive "uncaptured-reassigned" (uncaptured-reassigned))
(drive "immediate-init       " (immediate-init))
(drive "let-assign-inside    " (let-assign-inside))
(drive "let-assign-outside   " (let-assign-outside))
(drive "def-assign-inside    " (def-assign-inside))
(drive "def-assign-outside   " (def-assign-outside))
(drive "letrec-assign-outside" (letrec-assign-outside))
(drive "closure-init-tail    " (closure-init-tail))
(drive "alias-param          " (alias-param @[4 5 6]))
(drive "alias-local          " (alias-local))

(assert (= (let-assign-inside) 2) "the closure reads what it assigned")
(assert (= (let-assign-outside) 2) "the closure reads the assigned value")
(assert (= (def-assign-inside) 2) "the def closure reads what it assigned")
(assert (= (def-assign-outside) 2) "the def closure reads the assigned value")
(assert (= (letrec-assign-outside) 2)
        "the letrec closure reads the assigned value")
(assert (= (closure-init-tail) 2) "the tail call reads the assigned value")
(assert (= (alias-param @[4 5 6]) 4) "the aliased argument outlives the cell")
(assert (= (alias-local) 7) "the aliased local outlives the cell")

# ── a top-level cell over an alias ───────────────────────────────────
#
# Outside a function the binder mints a compiled cell. Churn between the
# `assign` and the read recycles a page the drop freed too early.

(defn churn []
  (def @k 0)
  (while (< k 64)
    (def junk @[k k k])
    (assign k (+ k 1))))

(def held @[1 2 3])
(def @top held)
(defn read-top []
  top)
(assign top 2)
(read-top)
(churn)
(assert (= (first held) 1) "a top-level def's alias init leaves its source")

(def kept @[4 5 6])
(let [@v kept
      g (fn [] v)]
  (assign v 2)
  (g))
(churn)
(assert (= (first kept) 4) "a top-level let's alias init leaves its source")

(println "region-capture-cell-init-drop: ok")
