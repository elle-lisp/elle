(elle/epoch 14)
# audited: 2026-10-06
# A tail call that passes a nested pair leaves standing every pair the callee builds.
#
# docs/impl/region/merging.md
# docs/impl/region/mechanism.md
#
# The release of the argument a callee received frees what the caller built,
# and nothing else. Each walk builds `(pair x (pair y acc))` and hands it to
# the next call. The two pairs merge into one region, so every activation
# allocates twice into one merged slot and releases the accumulator it
# received before its own tail call. The sidecar runs this file under
# `--trace=guardfree`, which faults on a read of a freed page.
#
# The counter-factuals are two faults that each looked like a correct list
# until something read it. A callee whose merged slot reused the caller's
# region built its pairs there, and the release of its argument freed them.
# A region walk that inlined the self call saw the two pairs of one activation
# hold each other, and freed both before the call that received them. The
# rows read every list after the recursion returns, so either fault reads a
# freed page.

# Hand-written intrinsics: the shape the region walk inlined into itself.
(defn walk-hand [lst acc]
  (if (%pair? lst)
    (walk-hand (%rest lst) (%pair (%first lst) (%pair 0 acc)))
    acc))
(assert (= (walk-hand (list 1 2 3) ()) (list 3 0 2 0 1 0)) "hand-written")

# The same, reading the accumulator through a call at the base.
(defn walk-rev [lst acc]
  (if (%pair? lst)
    (walk-rev (%rest lst) (%pair (%first lst) (%pair 0 acc)))
    (reverse acc)))
(assert (= (walk-rev (list 1 2 3) ()) (list 0 1 0 2 0 3)) "reversed at the base")

# The wrapper spelling, which the graft turns into the same nested %pair.
(defn walk-graft [lst acc]
  (if (empty? lst)
    (reverse acc)
    (walk-graft (rest lst) (pair (first lst) (pair 0 acc)))))
(assert (= (walk-graft (list 1 2 3) ()) (list 0 1 0 2 0 3)) "grafted")

# The stdlib function whose walker has this shape.
(assert (= (interpose :sep (list 1 2 3)) (list 1 :sep 2 :sep 3)) "interpose")

# A long list, so the pairs of many activations would share one region.
(def long (walk-graft (range 200) ()))
(assert (= (length long) 400) "two hundred activations")
(assert (= (get long 399) 199) "the last pair the last activation built")

# Two functions that call each other: each callee runs another body's slots.
(defn ping [lst acc]
  (if (%pair? lst)
    (pong (%rest lst) (%pair (%first lst) (%pair 0 acc)))
    acc))
(defn pong [lst acc]
  (if (%pair? lst)
    (ping (%rest lst) (%pair (%first lst) (%pair 1 acc)))
    acc))
(assert (= (ping (list 1 2 3 4) ()) (list 4 1 3 0 2 1 1 0)) "mutual recursion")
