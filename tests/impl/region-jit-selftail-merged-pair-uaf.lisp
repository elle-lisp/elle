(elle/epoch 15)
# audited: 2026-11-11
# A compiled self tail call that passes a nested pair leaves standing every pair the next trip builds.
# src/jit/AGENTS.md
#
# Each walker hands the next call `(%pair x (%pair 0 acc))`. The two pairs
# merge into one region, so each trip allocates twice into one merged slot and
# releases the accumulator it received. Compiled, the self tail call is a
# native loop inside one activation, and each trip must start with an empty
# region map, as a frame-replacing tail call starts the callee
# (docs/impl/region/merging.md).
#
# The counter-factual is a loop that keeps the map. The second trip's merged
# slot reuses the region the first trip minted, which now holds the moved
# accumulator, and the second trip's release of that accumulator frees the
# pairs it has just built there. The sidecar runs this file under guardfree,
# which faults at the first read of a freed page. The gauge reads the other
# side: a loop that dropped a region it should release leaks one per trip.
#
# The sidecar compiles each function on the VM thread at its first call. That
# call runs in the interpreter, so each face warms its walker first.

(defn walk-hand [lst acc]
  (if (%pair? lst)
    (walk-hand (%rest lst) (%pair (%first lst) (%pair 0 acc)))
    acc))

(defn walk-rev [lst acc]
  (if (%pair? lst)
    (walk-rev (%rest lst) (%pair (%first lst) (%pair 0 acc)))
    (reverse acc)))

(walk-hand () ())
(walk-rev () ())

(when (vm/config :jit)
  (assert (and (jit? walk-hand) (jit? walk-rev))
          "precondition: the walkers compile"))

(assert (= (walk-hand (list 1 2 3) ()) (list 3 0 2 0 1 0))
        "the compiled loop keeps every trip's pairs")
(assert (= (walk-rev (list 1 2 3) ()) (list 0 1 0 2 0 3))
        "the compiled loop keeps every trip's pairs, read through a call")

(def long (walk-rev (->list (range 200)) ()))
(assert (= (length long) 400) "two hundred trips")
(assert (= (get long 0) 0) "the first trip's inner pair")
(assert (= (get long 398) 0) "the last trip's inner pair")
(assert (= (get long 399) 199) "the last trip's element")

# ── the gauge ───────────────────────────────────────────────────────────────
(def window 200)

(defn measure []
  (def @i 0)
  (while (%lt i 20)
    (walk-rev (list 1 2 3 4) ())
    (assign i (%add i 1)))
  (def before (arena/count))
  (def @j 0)
  (while (%lt j window)
    (walk-rev (list 1 2 3 4) ())
    (assign j (%add j 1)))
  (%sub (arena/count) before))

(def d (measure))
(assert (%lt d 60) (string "the compiled loop leaks, delta=" d))

(println "region-jit-selftail-merged-pair-uaf: ok")
