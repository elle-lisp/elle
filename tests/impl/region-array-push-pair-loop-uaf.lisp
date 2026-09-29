(elle/epoch 12)
# audited: 2026-09-29
# A pair pushed into a let-bound array whose push result is discarded reclaims soundly in a loop.
# docs/impl/region/adopt.md
#
# The sidecar arms guardfree, so a double-free faults at the member's release.
#
# The `@[]` container is a `Fresh` call-result region; the pushed `%pair` is a store-adopted
# member of its Owned subtree (`AdoptRegion(container, pair)` at the push). The container's
# subtree drop is the pair's demise, and the pair keeps its OWN `DecrefRegion` — a structural
# no-op only while the pair is still `Owned`, so it must fire BEFORE that drop
# (docs/impl/region/adopt.md). At the let-body the pair's `decref_point` coincides with the
# container's: the container is freed by TWO releases there — its holder-binding release AND
# the discarded pass-through result of `%array-push` (which returns its container). Whichever
# zeroes the container triggers the drop. The counter-factual emits the pair's own decref
# after both (sorting the plain `DecrefRegion` last): the drop reclaims the pair first and
# the pair's slot-resolved decref then lands on a freed region — a phantom/double-free panic
# in the interpreter, a SIGSEGV under `--trace=guardfree`. The emit order's compile-level
# twin is
# `lir::lower::tests::release::store_adopted_member_release_precedes_owner_in_shared_bucket`.
#
# `%pair` lowers as the inline intrinsic, so the pair's member release is a
# slot-resolved `DecrefRegion` — exactly the release that faults on a freed region;
# `%array-push` is the native funnel call whose pass-through result doubles the
# container release.

# The trigger shape: the push result is discarded (the `let` is not the loop body's
# tail), so the container is released by both its binding and the discarded pass-through
# result. A double-free faults here if the member's release comes last.
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
    # `get` returns unknown, so prove the %first/% add operands with a
    # %pair?/%int? dispatch; the read-back-through-the-container shape (the
    # pin) is unchanged.
    (let [p (get items 0)]
      (when (%not (%pair? p)) (error :not-a-pair))
      (let [car (%first p)]
        (when (%not (%int? car)) (error :not-an-int))
        (assign sum (%add sum car)))))
  (assign k (%add k 1)))
(assert (= sum 19900) "sum of pushed cars for k in 0..200 must be 19900")

(println "region-array-push-pair-loop-uaf: ok")
