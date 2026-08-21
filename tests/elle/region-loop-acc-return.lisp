(elle/epoch 12)
# A function accumulates into a fn-local mutable across a `while` and hands the
# final content back. The cell takes a COUNTED reference of its own at every
# store, so each value the loop displaces dies at the overwrite and the last one
# leaves with the caller: the `Return` mints the caller's reference and the
# cell's content drop, emitted after that mint, releases the cell's
# (docs/impl/region/bindings.md § "Returned fn-local reassigned mutables — the
# return claims the MINT's reference, not the cell's").
#
# Withholding the container half from a returned binding leaves each stored
# value protected only by its producer's reference, whose one release rides the
# returned-region extension out to the `Return` — where it names whatever the
# producer's ANF slot holds LAST. Every earlier value the loop stored is then
# stranded: one region per trip, growing with the trip count.
#
# Three faces, because the two failure modes point opposite ways: releasing the
# content while the caller still holds it frees a live value, and releasing a
# displaced prior nowhere strands one region per iteration.

(defn acc-return [n]
  (var acc ())
  (var i 0)
  (while (%lt i n)
    (assign acc (pair (string "e" i) acc))
    (assign i (%add i 1)))
  acc)

# ── 1. Correctness — the returned content is what the loop built ────────
(assert (empty? (acc-return 0)) "an unrun loop returns the init value")
(assert (= (length (acc-return 4)) 4) "one element per trip")
(assert (= (first (acc-return 4)) "e3") "the head is the last value stored")
(assert (= (first (rest (acc-return 4))) "e2") "the tail is the prior value")

# ── 2. Not over-freed — the caller's reference outlives the callee ──────
# The callee drops the cell's reference at its own scope demise; were that drop
# to consume the caller's reference instead, the returned chain would be freed
# under the reads below. Allocation churn between the call and the reads makes a
# freed page likely to be recycled, so a stale read shows as a wrong value
# rather than as intact bytes.
(def held (acc-return 6))
(def junk @[])
(each i in (range 0 64)
  (push junk (string "junk" i)))
(assert (= (length held) 6) "the returned chain survives the callee's release")
(assert (= (first held) "e5") "the returned head is intact after churn")
(assert (= (first (rest (rest held))) "e3") "an interior element is intact")

# ── 3. Bounded — the rate is flat in the trip count ─────────────────────
# Each iteration's value dies at the next overwrite, so a driver that reclaims
# measures the same growth at n=4 and n=16. A driver that strands the displaced
# priors grows with n.
(defn drive [reps n]
  (var k 0)
  (while (%lt k reps)
    (length (acc-return n))
    (assign k (%add k 1))))

(defn growth [reps n]
  (drive 20 n)
  (var before (arena/region-count))
  (drive reps n)
  (%sub (arena/region-count) before))

(let [small (growth 200 4)
      large (growth 200 16)]
  (assert (%lt small 100)
          (string "a returned loop accumulator strands its displaced priors: "
                  "live count grew by " small " over 200 calls at n=4 "
                  "(expected flat)"))
  (assert (%lt large (%add small 100))
          (string "the strand scales with the trip count: " small " at n=4 vs "
                  large " at n=16 over 200 calls")))

(println "region-loop-acc-return: ok")
