(elle/epoch 12)
# audited: 2026-09-21
# A dispatch stores into one fn-local mutable from each of its arms, and nothing
# reads the binding afterward. The cell's content drop is one point, so it must
# be a point every storing path reaches: the cell's own SCOPE NODE, which
# contains every write by construction (docs/impl/region/bindings.md § "Where the
# content drop lands").
#
# Placing the drop at the latest WRITE instead puts it inside one arm. Every
# other arm then stores a value the cell's counted reference holds and nothing
# releases — one region per call, for as long as the program runs. The reader
# that would otherwise carry the drop past the dispatch is what hides this, which
# is why the shape has none.
#
# The trap is the intervening binder. Functionalization gives a
# conditionally-assigned mutable a phi read after the branch when the branch sits
# directly in the binding's own scope, and that read carries the drop out on its
# own. Wrap the dispatch in a `let` of its own — what `each` does with the
# collection it walks — and the phi is gone, so only the scope-node floor is left
# to place the drop.
#
# Three faces: the dispatch computes what it should, no arm's value is freed
# while the cell holds it, and the rate is flat per call.

(def @picker @[:a])

# The shape. `k` is the intervening binder; every arm's loop stores a fresh list.
(defn dispatch []
  (var u nil)
  (let [k (get picker 0)]
    (match k
      :a
        (begin
          (var j 0)
          (while (%lt j 2)
            (assign u (list j))
            (assign j (%add j 1))))
      :b
        (begin
          (var m 10)
          (while (%lt m 12)
            (assign u (list m))
            (assign m (%add m 1))))
      _ nil))
  nil)

# The same dispatch with the cell READ after it — the control whose phi read
# places the drop without the floor. Bounded before this shape was.
(defn dispatch-read []
  (var u nil)
  (let [k (get picker 0)]
    (match k
      :a
        (begin
          (var j 0)
          (while (%lt j 2)
            (assign u (list j))
            (assign j (%add j 1))))
      :b
        (begin
          (var m 10)
          (while (%lt m 12)
            (assign u (list m))
            (assign m (%add m 1))))
      _ nil))
  (if (nil? u) 0 (first u)))

# ── 1. Correctness — each arm runs and leaves the value it stored ────────
(assert (nil? (dispatch)) "the dispatch answers nil whichever arm ran")
(assert (= (dispatch-read) 1) "the :a arm's last iteration stored 1")
(put picker 0 :b)
(assert (= (dispatch-read) 11) "the :b arm's last iteration stored 11")
(put picker 0 :c)
(assert (= (dispatch-read) 0) "the fall-through arm stores nothing")
(put picker 0 :a)

# ── 2. Not over-freed — an arm's value survives while the cell holds it ──
# Each stored value also goes into a keeper array, a runtime-counted funnel
# store, so a drop that ran on an arm the path did not take — freeing a value the
# cell still holds — would be read back here as a stale element.
(defn dispatch-keep []
  (var out @[])
  (var u nil)
  (let [k (get picker 0)]
    (match k
      :a
        (begin
          (var j 0)
          (while (%lt j 2)
            (assign u (list j))
            (%array-push out u)
            (assign j (%add j 1))))
      _ nil))
  (%freeze out))

(def kept (dispatch-keep))
(assert (= (length kept) 2) "the keeper holds one value per iteration")
(assert (= (first (get kept 0)) 0) "iteration 0's value is intact")
(assert (= (first (get kept 1)) 1) "iteration 1's value is intact")

# ── 3. Bounded — the rate is flat per call ──────────────────────────────
# The arm's last value dies at the cell's content drop, so a dispatch that
# reclaims measures the same growth over 200 calls as over 400. A dispatch whose
# drop sits in an arm it never takes grows with the call count.
(defn drive [reps f]
  (var k 0)
  (while (%lt k reps)
    (f)
    (assign k (%add k 1))))

(defn growth [reps f]
  (drive 20 f)
  (var before (arena/region-count))
  (drive reps f)
  (%sub (arena/region-count) before))

(let [small (growth 200 dispatch)
      large (growth 400 dispatch)]
  (assert (%lt small 100)
          (string "a dispatch arm's stored value is stranded: live count grew "
                  "by " small " over 200 calls (expected flat)"))
  (assert (%lt large (%add small 100))
          (string "the strand scales with the call count: " small
                  " over 200 calls vs " large " over 400")))

# The control was bounded before the floor landed, so a regression that moves
# only this number is a different defect from the one above.
(assert (%lt (growth 200 dispatch-read) 100)
        "the dispatch whose cell is read afterward is bounded")

(println "region-cell-arm-demise: ok")
