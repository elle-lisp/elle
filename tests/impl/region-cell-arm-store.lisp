(elle/epoch 12)
# A branch inside a loop stores into ONE reassigned mutable from both of its
# arms, so the cell records two store sites and each arm allocates the value it
# stores. Each stored value's producer reference is discharged at the store that
# took THAT value — the cell's own counted reference keeps the value from there
# on (docs/impl/region/bindings.md § "The store site is the store that took THAT
# value").
#
# Pinning both values at the cell's LAST store instead puts the first arm's
# release inside the SECOND arm, which the first arm's path does not reach. An
# iteration that takes the first arm again then displaces the previous value from
# its own ANF slot before the pin ever runs, so that value's producer reference is
# stranded: one region per repeat, growing with the iteration count.
#
# Three faces, because the two failure modes point opposite ways. Releasing a
# stored value while the cell still holds it frees a live value; releasing it
# nowhere strands one region per repeated arm.

(defn arms [n]
  (var last (array 0 0))
  (var i 0)
  (while (%lt i n)
    (if (%lt (%mod i 4) 2) (assign last (array i 7)) (assign last (array i 9)))
    (assign i (%add i 1)))
  (get last 1))

# ── 1. Correctness — the cell's content survives every release ──────────
# `(arms 0)` runs no iteration, so the init value is read back; every other
# count reads back the arm the last iteration took.
(assert (= (arms 0) 0) "an unrun loop reads the init value back")
(assert (= (arms 1) 7) "iteration 0 takes the first arm")
(assert (= (arms 3) 9) "iteration 2 takes the second arm")
(assert (= (arms 6) 7) "iteration 5 takes the first arm again")

# ── 2. Not over-freed — every value the loop produced stays readable ────
# Each stored value also goes into a keeper array (a runtime-counted funnel
# store), so a value released while the cell still holds it would be freed under
# a live holder.
(defn arms-keep [n]
  (var out @[])
  (var last (array 0 0))
  (var i 0)
  (while (%lt i n)
    (if (%lt (%mod i 4) 2) (assign last (array i 7)) (assign last (array i 9)))
    (%array-push out last)
    (assign i (%add i 1)))
  (%freeze out))

(def kept (arms-keep 6))
(assert (= (length kept) 6) "the keeper holds one value per iteration")
(assert (= (get (get kept 0) 1) 7) "iteration 0's value is intact")
(assert (= (get (get kept 1) 0) 1) "iteration 1's value is intact")
(assert (= (get (get kept 2) 1) 9) "iteration 2's value is intact")
(assert (= (get (get kept 3) 1) 9) "iteration 3's value is intact")
(assert (= (get (get kept 5) 1) 7) "iteration 5's value is intact")

# ── 3. Bounded — the rate is flat in the iteration count ────────────────
# Each iteration's value dies at the next overwrite, so a loop that reclaims
# measures the same growth at n=8 and n=16. A loop that strands the repeated
# arm's value grows with n.
(defn drive [reps n]
  (var k 0)
  (while (%lt k reps)
    (arms n)
    (assign k (%add k 1))))

(defn growth [reps n]
  (drive 20 n)
  (var before (arena/region-count))
  (drive reps n)
  (%sub (arena/region-count) before))

(let [small (growth 200 8)
      large (growth 200 16)]
  (assert (%lt small 400)
          (string "a branch arm's stored value is stranded: live count grew by "
                  small " over 200 calls at n=8 (expected flat)"))
  (assert (%lt large (%add small 100))
          (string "the strand scales with the iteration count: " small
                  " at n=8 vs " large " at n=16 over 200 calls")))

(println "region-cell-arm-store: ok")
