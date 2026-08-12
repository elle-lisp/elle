(elle/epoch 12)
# A fn-local 1-slot container whose INIT value carries a second name. The cell
# cannot take that value by donation — the alias holds the producer's reference
# and reads through it after the loop — so it counts the init instead, and keeps
# the rest of the container model with it (docs/impl/region/bindings.md § "What
# the cell donates it must hold alone; what it counts it need not").
#
# What the model is worth here is the STORE-SITE PIN. Without it each stored
# value's producer release rides the cell binding's uses out past the loop, so a
# loop that stores n values releases one and strands n-1. The cursor walk below
# is the everyday shape: `(assign r (rest r))` over a cons chain, where every
# step's stored value lives in the head's region and one release per call leaves
# the whole chain live.

# ── 1. The cursor walk — the incoming value comes OUT of the displaced one ──
# `xs` names the chain's head for the whole call, so the cell's init is aliased.
# Each `(rest r)` hands back a borrow living in the head's region, and its return
# mint is the producer reference the store site releases.
(defn walk-count [xs]
  (var r xs)
  (var n 0)
  (while (not (empty? r))
    (assign n (%add n 1))
    (assign r (rest r)))
  n)

(defn cursor [k]
  (let [xs (list 1 2 3 4)]
    (walk-count xs)))

(assert (= (cursor 0) 4) "the cursor visits every cons of the chain")

# ── 2. Not over-freed — the alias outlives every overwrite ──────────────
# `xs` is read AFTER the loop has displaced the head several times. A cell that
# took the init by donation would have released the alias's reference at its
# first overwrite and this read would walk a reclaimed page.
(defn cursor-read-back [n]
  (let [xs (list 1 2 3 4 5)]
    (var r xs)
    (var seen 0)
    (while (not (empty? r))
      (assign seen (+ seen (first r)))
      (assign r (rest r)))
    (list seen (first xs) (length xs))))

(assert (= (cursor-read-back 0) (list 15 1 5))
        "the aliased init stays readable after the cursor has walked off it")

# ── 3. Fresh stored values — each displaced prior dies at the overwrite ──
# The same cell shape with an unrelated value per iteration. Here the strand is
# the stored value's own region, one per iteration rather than one per call.
(defn churn [n]
  (let [xs (array 0 0)]
    (var last xs)
    (var i 0)
    (while (< i n)
      (assign last (array i 7))
      (assign i (+ i 1)))
    (list (get xs 1) (get last 1))))

(assert (= (churn 0) (list 0 0))
        "an unrun loop reads the init back through both names")
(assert (= (churn 4) (list 0 7))
        "the last stored value reads back beside the aliased init")

# ── 4. Bounded — the rate is flat in the step count ─────────────────────
# A cursor that strands one region per call and a churn that strands one per
# iteration both grow here; a model that pins each release to its store site
# measures the same growth at n and at 2n.
(defn drive-cursor [reps]
  (var k 0)
  (while (< k reps)
    (cursor k)
    (assign k (+ k 1))))

(defn drive-churn [reps n]
  (var k 0)
  (while (< k reps)
    (churn n)
    (assign k (+ k 1))))

(defn growth [f]
  (f 20)
  (var before (arena/region-count))
  (f 200)
  (%sub (arena/region-count) before))

(let [cursor-rate (growth (fn [reps] (drive-cursor reps)))
      small (growth (fn [reps] (drive-churn reps 8)))
      large (growth (fn [reps] (drive-churn reps 16)))]
  (assert (%lt cursor-rate 100)
          (string "the cursor strands its chain: live count grew by "
                  cursor-rate " over 200 calls (expected flat)"))
  (assert (%lt small 100)
          (string "the churn strands its stored values: live count grew by "
                  small " over 200 calls at n=8 (expected flat)"))
  (assert (%lt large (%add small 100))
          (string "the churn's rate scales with the iteration count: " small
                  " at n=8 vs " large " at n=16 over 200 calls")))

(println "region-cell-aliased-init: ok")
