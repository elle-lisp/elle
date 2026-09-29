(elle/epoch 12)
# A `@`-mutable local a closure captures costs one env cell per activation,
# and that cell is released whichever binder introduced it.
#
# Such a local lives in the activation's environment: `populate_env` mints one
# `CaptureCell` for it, in a region of its own, once per call. Nothing frees
# that region unless the compiler emits a release for it, and the release is
# armed by a placeholder the region walk records against the binding.
#
# ── The shape ───────────────────────────────────────────────────────
#
# The binder decides nothing about the cell — `def @v` and `let [@v …]` inside
# a lambda are the same env cell, minted the same way — so a placeholder
# recorded for one binder and not the other is a leak in whichever binder is
# missed, at one region and one object per activation per such local.
#
# This is the closure-as-module idiom's cost: a constructor that returns a
# struct of closures over its own mutable state pays it once per field, on
# every construction. lib/http2/stream.lisp's `make-channel` is two.
#
# ── Reading the numbers ─────────────────────────────────────────────
#
# Each drive runs the same construction at `calls` iterations after an
# identical warmup, so a per-call cost reads `calls` (or a multiple) and a
# one-off reads about 0. The controls fix the window's own bookkeeping cost,
# which is what `slack` is sized for.

(def calls 200)
(def slack 20)

# ── controls: no env cell to release ─────────────────────────────────

(defn immutable-capture []
  (let [v 1]
    {:read (fn [] v)}))

(defn uncaptured-mutable []
  (let [@v 1]
    (assign v 2)
    v))

# ── subjects: one env cell per captured @-mutable local ──────────────

(defn def-bound []
  (def @v 1)
  {:set (fn [] (assign v 2))})

(defn let-bound []
  (let [@v 1]
    {:set (fn [] (assign v 2))}))

(defn let-bound-two []
  (let [@v 1
        @w 2]
    {:set (fn []
            (assign v 2)
            (assign w 3))}))

# The closure never leaves the call, so nothing outside can be holding the
# cell when the activation ends.
(defn let-bound-local []
  (let [@v 1]
    ((fn [] (assign v 2)))
    v))

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

(println "region-let-capture-cell-leak over " calls " calls:")

(drive "immutable-capture " (immutable-capture))
(drive "uncaptured-mutable" (uncaptured-mutable))
(drive "def-bound         " (def-bound))
(drive "let-bound         " (let-bound))
(drive "let-bound-two     " (let-bound-two))
(drive "let-bound-local   " (let-bound-local))

# The cell must still be readable and writable for the whole activation: a
# release that fired early would be caught by section 4 of
# region-capture-cell-loop-uaf.lisp, and one that never fires is the leak
# above, so this pins the behaviour the two bound between them.

(let [m (let-bound)]
  (m:set)
  (assert (= (m:set) 2) "the captured cell is still writable after the call"))

(println "region-let-capture-cell-leak: ok")
