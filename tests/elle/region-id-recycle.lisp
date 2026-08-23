(elle/epoch 12)
# What a call costs in region IDS — the dimension no other gauge sees
# (docs/impl/region/model.md § "Physical id recycling").
#
# Every native call mints a physical region for its result, before the callee
# runs, because the callee may allocate the result into it. A primitive that
# returns an immediate, or one that returns a value borrowed from an argument,
# allocates nothing into that region — so the id never becomes a live region.
#
# Such an id holds no object, no page, and no reference count. `arena/count`,
# `arena/region-count`, `arena/bytes`, and `arena/page-claims` are therefore all
# blind to it: they read perfectly flat while the id is stranded.
#
# `arena/region-ids` is the gauge that sees it — `next_physical`, one past the
# largest id ever minted from scratch. A mint that finds an id on the free list
# leaves it alone, so a steady-state loop holds it flat and every unit of growth
# is an id that did not come back.
#
# `arena/region-table` is NOT the gauge to assert on, and the difference is the
# trap this file exists to avoid. The table only grows when an id is made LIVE,
# and a stranded id never is: a loop of calls that allocate nothing can leak an
# id per call and leave the table flat for the whole run. Measured here as a
# second reading, never as a ceiling.
#
# Every bound below is a CEILING, so the file is shrink-only.

(def window 4000)

(defn ids-issued [thunk warm window]
  "Physical region ids issued over WINDOW calls of THUNK, after WARM untimed calls."
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/region-ids))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/region-ids) before))

# the gauge-live discriminator ─────────────────────────────────────────────────
# A ceiling reads green against a dead gauge too, and a gauge frozen at whatever
# the stdlib load left behind would pass every assertion below. So make the
# counter MOVE first, and read the move.
#
# It only rises when a program holds more ids at once than it ever has, so hold
# a margin past the current reading: every cons in the array is a live region
# holding a live id, and the counter cannot answer the same number afterwards.

(def base (arena/region-ids))
(def hold (@array))
(var k 0)
(while (%lt k (%add base 5000))
  (push hold (pair k k))
  (assign k (%add k 1)))
(def live-ids (arena/region-ids))
(println "region-id-recycle: " (length hold)
         " live conses moved region-ids from " base " to " live-ids)
(assert (%gt live-ids base)
        (concat "arena/region-ids is dead: holding "
                (number->string (length hold))
                " live conses at once left it at " (number->string live-ids)
                ", where it already was — every ceiling below is void"))

# measurements ─────────────────────────────────────────────────────────────────
# Each subject is a call whose result is NOT allocated in the call's own region,
# so the region it mints stays unmaterialized and must come straight back.

(def imm-d (ids-issued (fn [] (< 1 2)) 200 window))
(def len-d (ids-issued (fn [] (length hold)) 200 window))
(def get-d (ids-issued (fn [] (get hold 0)) 200 window))
(def first-d (ids-issued (fn [] (first (pair 1 2))) 200 window))
(def fresh-d (ids-issued (fn [] (pair 1 2)) 200 window))

(println "  immediate result: (< 1 2) " imm-d "  (length a) " len-d)
(println "  borrowed result:  (get a 0) " get-d "  (first p) " first-d)
(println "  fresh result:     (pair 1 2) " fresh-d)

(defn at-most [d ceiling label]
  (assert (%le d ceiling)
          (concat label " issued " (number->string d) " new region ids over "
                  (number->string window) " calls, over the ceiling of "
                  (number->string ceiling)
                  " — its minted ids are not returning to the free list")))

# A ceiling of zero admits nothing: a loop of calls that allocate nothing must
# leave the counter exactly where it found it, at any iteration count.
(at-most imm-d 0 "(< 1 2), whose result is an immediate")
(at-most len-d 0 "(length a), whose result is an immediate")
(at-most get-d 0 "(get a 0), whose result is borrowed from the array")
(at-most first-d 0 "(first p), whose result is borrowed from the pair")

# A call that DOES allocate its result is bounded the same way, by the other
# half of the contract: the result region dies at its decref_point and the
# teardown recycles its id.
(at-most fresh-d 0 "(pair 1 2), whose result region dies at its decref_point")

# The resident cost of the same contract. The table is indexed by physical id,
# so it can only stay bounded if the ids do; with the ids flat above, the table
# must be flat too.
(def table-before (arena/region-table))
(var m 0)
(while (%lt m window)
  (first (pair 1 2))
  (assign m (%add m 1)))
(def table-growth (%sub (arena/region-table) table-before))
(println "  region table over " window " more calls: " table-growth " entries")
(assert (%le table-growth 0)
        (concat "the region table grew by " (number->string table-growth)
                " entries over " (number->string window)
                " calls — resident memory per call, invisible to every other gauge"))

(println "region-id-recycle: ok")
