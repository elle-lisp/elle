(elle/epoch 12)
# A whole-value read of a fn-local 1-slot container, taken AFTER the container's
# own binder allocated the init. The reader takes a COUNTED reference of its own
# — the container releases what it held at every overwrite, so an uncounted
# borrow dies at the first `assign` (docs/impl/region/bindings.md § "A whole-value
# read of a 1-slot container takes a counted reference").
#
# What the counted read is worth here is the DONATION. The reader holds a
# reference of its own, so the cell is the init's sole holder and takes it
# uncounted, released by drop-on-overwrite. The alternative — the cell counting
# its init while the reader keeps the producer's reference — leaves that
# reference to be released through the slot recorded for the init region, which
# in THIS ordering is the cell's own. A mutated slot is no release route, so the
# producer's reference would have none and the whole chain would strand once per
# call.
#
# tests/elle/region-cell-aliased-init.lisp is the same program with the alias
# bound FIRST: there the alias allocates, its own slot carries the release, and
# the counted-init route runs instead. The two files together isolate the
# ordering rather than the model.

# ── 1. The cursor walk, alias taken after the cell ──────────────────────
# `keep` names the chain's head; the cursor then walks `r` off it. The read-back
# is what a donated-but-unaliased init would fail: the first overwrite would have
# released the only reference standing under `keep`.
(defn cursor-after [k]
  (var r (list 1 2 3 4 5))
  (let [keep r]
    (var seen 0)
    (while (not (empty? r))
      (assign seen (+ seen (first r)))
      (assign r (rest r)))
    (list seen (first keep) (length keep))))

(assert (= (cursor-after 0) (list 15 1 5))
        "the alias reads back the chain head the cursor walked off")

# ── 2. The cell bound by a `let`, the reader by a nested `let` ──────────
# The same shape with both names bound by `let` rather than at the function
# body's top. The container model does not read the binder form, so this must
# reclaim exactly as face 1 does.
(defn cursor-after-let [k]
  (let [@r (list 1 2 3 4)]
    (let [keep r]
      (var n 0)
      (while (not (empty? r))
        (assign n (+ n 1))
        (assign r (rest r)))
      (list n (first keep)))))

(assert (= (cursor-after-let 0) (list 4 1))
        "the let-bound alias reads back its head after the walk")

# ── 3. Unrelated stored values — the displaced init dies at the overwrite ──
# Each iteration stores a fresh array, so the init and its replacements live in
# regions of their own and the alias's reference is the init's only protection
# once the cell has moved on.
(defn churn-after [n]
  (let [@last (array 41 42)]
    (let [keep last]
      (var i 0)
      (while (< i n)
        (assign last (array i 7))
        (assign i (+ i 1)))
      (list (get keep 1) (get last 1)))))

(assert (= (churn-after 0) (list 42 42))
        "an unrun loop reads the init back through both names")
(assert (= (churn-after 4) (list 42 7))
        "the alias survives every overwrite the loop makes")

# ── 4. Control — the same names bound by `def` rather than by `let` ─────
# Functionalization promotes a `def`-bound reassigned mutable to a loop
# parameter, so the chain's source binder is a name the loop never repoints and
# its slot carries the release. Bounded with or without the counted read; the gap
# between this face and face 3 is the binder form, not the model.
(defn churn-def [n]
  (var last (array 41 42))
  (var keep last)
  (var i 0)
  (while (< i n)
    (assign last (array i 7))
    (assign i (+ i 1)))
  (list (get keep 1) (get last 1)))

(assert (= (churn-def 4) (list 42 7))
        "the def-bound alias survives every overwrite the loop makes")

# ── 5. Bounded ─────────────────────────────────────────────────────────
# Each face strands its whole object graph per call when the producer's reference
# has no release route: the cursor's cons chain, the churn's displaced init.
(defn drive [f reps]
  (var k 0)
  (while (< k reps)
    (f k)
    (assign k (+ k 1))))

(defn growth [f]
  (drive f 20)
  (var before (arena/region-count))
  (drive f 200)
  (%sub (arena/region-count) before))

(let [cursor-rate (growth (fn [k] (cursor-after k)))
      let-rate (growth (fn [k] (cursor-after-let k)))
      churn-rate (growth (fn [k] (churn-after 4)))
      def-rate (growth (fn [k] (churn-def 4)))]
  (assert (%lt cursor-rate 100)
          (string "the cursor strands its chain: live count grew by "
                  cursor-rate " over 200 calls (expected flat)"))
  (assert (%lt let-rate 100)
          (string "the let-bound cursor strands its chain: live count grew by "
                  let-rate " over 200 calls (expected flat)"))
  (assert (%lt churn-rate 100)
          (string "the churn strands its displaced init: live count grew by "
                  churn-rate " over 200 calls (expected flat)"))
  (assert (%lt def-rate 100)
          (string "the def-bound control regressed: live count grew by "
                  def-rate " over 200 calls (expected flat)")))

(println "region-cell-alias-after: ok")
