(elle/epoch 12)
# A fresh `%pair` pushed into a fresh, let-bound `@[]` whose push result is
# DISCARDED, driven in a loop, must reclaim its Owned subtree soundly — no
# double-free.
#
# The `@[]` container is a `Fresh` call-result region; the pushed `%pair` is a
# store-adopted member of its Owned subtree (`AdoptRegion(container, pair)` at the
# push). The container's subtree drop is the pair's demise, and the pair keeps its
# OWN `DecrefRegion` — a structural no-op only while the pair is still `Owned`, so it
# must fire BEFORE that drop (docs/impl/region-model.md § "The lifetime obligation the
# root carries"). At the let-body the pair's `decref_point` coincides with the
# container's: the container is freed by TWO releases there — its holder-binding release
# AND the discarded pass-through result of `%array-push` (which returns its container).
# Whichever zeroes the container triggers the drop; if the pair's own decref is emitted
# after both (the pre-fix bucket order, which sorted the pair's plain `DecrefRegion`
# last), the drop reclaims the pair first and the pair's slot-resolved decref then lands
# on a freed region — a phantom/double-free panic on the plain VM, a SIGSEGV under
# `--trace=guardfree`. Pinned by `region_array_push_pair_loop_uaf`
# (tests/integration/elle_scripts.rs) under the UAF oracle; the emit-order fix is
# `lir::lower::tests::release::store_adopted_member_release_precedes_owner_in_shared_bucket`.
#
# Only `--checked-intrinsics=off` reaches the trigger: there `%pair` is an intrinsic
# whose member release is a slot-resolved `DecrefRegion` (faults on a freed region),
# where checked-on it is a `Fresh` value-based release. The `elle test` runner covers
# both settings (vm=checked-on, jit=checked-off); this file also runs under
# `--jit=adaptive` (checked-off) as the deterministic-fault oracle.

# The trigger shape: the push result is discarded (the `let` is not the loop body's
# tail), so the container is released by both its binding and the discarded pass-through
# result. A double-free faults here on the pre-fix bucket order.
(def @i 0)
(while (%lt i 200)
  (let [items @[]]
    (%array-push items (%pair 1 2)))
  (assign i (%add i 1)))

# Correctness: the same shape, but the pushed pair must read back intact each
# iteration (an early free would corrupt the read or fault). Accumulate the read-back
# cars so a torn read shows as a wrong sum, not only as a crash.
(def @sum 0)
(def @k 0)
(while (%lt k 200)
  (let [items @[]]
    (%array-push items (%pair k 0))
    (assert (= 1 (length items)) (string "push lost at k=" k))
    (assign sum (%add sum (%first (get items 0)))))
  (assign k (%add k 1)))
(assert (= sum 19900) "sum of pushed cars for k in 0..200 must be 19900")

(println "region-array-push-pair-loop-uaf: ok")
