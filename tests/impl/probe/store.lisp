(elle/epoch 15)
# audited: 2026-11-17
# The physical-id dimension no other gauge shows, and the remove/rebind half of the mutable-store funnel.
#
# docs/impl/region/diagnostics.md
# ── The physical-id dimension ─────────────────────────────────────────
# What a CALL costs in physical region ids, the dimension every other probe is
# blind to. A native call mints a physical region for its result before the
# callee runs, because the callee may allocate the result into it. A callee
# that returns an immediate, or a value borrowed from an argument, allocates
# nothing into that id. It never becomes a live region, so no teardown can
# return it. It holds no object, no page, no bytes and no reference count for
# any other gauge to see (docs/impl/region/model.md).
#
# What it costs is resident: the region table is a `Vec` indexed by physical id,
# so the largest id ever made live sets its length. Both exits of the id
# lifecycle are gauged here, and the pair must stay together. The recycle admits
# an id only where the mint never materialized it, so an id that DID materialize
# reaches the free list by its teardown instead. A probe of one exit alone cannot
# tell a working recycle from one that hands the same id back twice.
#
# `id-immediate-result` and `id-const-compare` are results a native never
# allocates at all. `id-borrowed-element` and `id-borrowed-index` are results
# borrowed out of an argument. `id-fresh-result` is the materializing control,
# whose id comes back by the ordinary teardown. These are controls at 0. The
# instrument proves the id gauge live ahead of the first of them, because an id
# gauge that cannot move reads 0 for all five.
(def id-hold [1 2 3])
(println "── folded suite: physical-id recycling ──")
(r:drive "id-const-compare" (r:stmt-run (fn [j] (< 1 2))) :on [r:ids])
(r:drive "id-immediate-result" (r:stmt-run (fn [j] (length id-hold)))
         :on [r:ids])
(r:drive "id-borrowed-index" (r:stmt-run (fn [j] (get id-hold 0))) :on [r:ids])
(r:drive "id-borrowed-element" (r:stmt-run (fn [j] (first (pair 1 2))))
         :on [r:ids])
(r:drive "id-fresh-result" (r:stmt-run (fn [j] (pair 1 2))) :on [r:ids])

# ── The mutable-store funnel — remove/rebind half ─────────────────────
# The store half (push/put/add) is read in probe/direct.lisp and
# probe/container.lisp (push-churn/struct-put/set-array/…). These read the
# REMOVE and REBIND half of the same seam (the outgoing edge table of
# docs/impl/region/ownership.md; src/value/arena/mutate.rs). The funnel SEAM is
# complete by construction. Every remove co-locates its RC decref with the
# outgoing un-record, and the raw accessors are private, so an uncounted store
# is a compile error. A debug equivalence oracle asserts the recorded table
# matches a content scan at every free. These rows read the seam THROUGH the
# surface that reaches it, and split cleanly.
#
#   `%pop`: the remove funnel balances (rate 0). A box store+rebind and `%pop`'s
#   `moves_out` native each reclaim their cross-region member. So `raw-pop` is
#   the reclaiming CONTROL, the peer of push-slot-source/put-slot-source, and a
#   wrapper that leaks over it is the wrapper's leak. It is a DIRECT
#   while-statement, not a thunk, so the popped value is discarded as a
#   statement. That isolates the remove funnel's own reclamation from the return
#   convention, and from how the ownership forest handles a value pushed into a
#   LOCAL, popped OUT and RETURNED. `%pop`'s result is a distinct `call_result`
#   region with its own `DecrefValueRegion`, which balances the `moves_out`
#   retain (`pop_with_decref`) that hands the element back.
#
#   remove-wrapper: the stdlib `pop`/`del` `(match (type-of coll) …)` dispatch
#   wrapper strands the container arg and fresh result on the arms the
#   textually-last arm does not reach, as the STORE wrappers (put/push/set) do.
#   The SAME mechanism as the store half reclaims them: per-arm compensation of
#   the container and result, or dispatch prune on a statically-typed scrutinee.
#
#   The RAW remove funnel reclaims too (`raw-del`/`raw-del-immediate` = 0).
#   `%del`'s in-place / remove decrefs the removed member, and its
#   `-mut` pass-through result carries exactly one return mint. These two are the
#   CLOSED raw-funnel controls for the remove half, the peers of
#   `raw-pop`/`put-slot-source`. Each body is two statements whose tail is the
#   funnel call: the ANF-named tail call whose result one `Return` mint covers
#   (docs/impl/region/mechanism.md). A second, unbalanced retain there reads as a
#   whole stranded container plus the member it holds.
(println "── folded suite: mutable-store funnel (remove/rebind half) ──")
(r:drive "box-rebind"
         (r:stmt-run (fn [j]
                       (let [b (box (list 1 2))]
                         (rebox b (list 3 4))))))
# The stdlib `add` `(match (type-of coll) …)` dispatch
# wrapper reclaims its owned @set container AND its stored heap member (rate 0): the
# `:@set` arm's `%add-set-mut` returns the container pass-through, and the wrapper's
# per-arm container release (`container_release_sites`,
# src/hir/region/infer/compensate.rs) frees the stranded owned-param reference,
# cascading the stored list through the outgoing edge table. A CLOSED control beside
# the reclaiming raw funnel `set-add-slot-source`; it opens if the container
# compensation regresses.
(r:drive "set-add"
         (r:stmt-run (fn [j]
                       (let [s @||]
                         (add s (list 1 2))))))
(r:drive "raw-pop"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (def @j 0)
           (while (%lt j b)
             (let [a @[]]
               (%array-push a (%pair 1 2))
               (%pop a))
             (assign j (%add j 1)))))
# The stdlib `pop` REMOVE-of-ELEMENT wrapper reclaims (rate 0). Its
# `:@array`/`:@string`/`:@bytes` arms route to the monomorphic moves-out
# funnels `%pop`/`%pop-string`/`%pop-bytes`. The container compensation frees the
# wrapper's stranded owned-param container per arm, recorded for a moves-out
# funnel even though it returns the ELEMENT. And the moved-out @array element's
# redundant tail ReturnValue retain is suppressed (`moves_out_release_sites`).
# The two together reclaim both the container and the element.
(r:drive "pop-wrapper"
         (r:stmt-run (fn [j]
                       (let [a @[]]
                         (push a (list 1 2))
                         (pop a)))))
# The stdlib `del` REMOVE wrapper reclaims (rate 0), the remove-half peer of the
# store wrappers. Its `:@struct`/`:@set` arms route to the `-mut` remove funnels
# (`%del-struct-mut`/`%del-set-mut`) that return the container pass-through. The
# wrapper's container compensation frees the stranded owned-param reference. A
# CLOSED control beside the reclaiming raw funnel `put-slot-source`.
(r:drive "del-wrapper"
         (r:stmt-run (fn [j]
                       (let [m @{}]
                         (put m :k (list 1 2))
                         (del m :k)))))
(r:drive "set-del-wrapper"
         (r:stmt-run (fn [j]
                       (let [s @||]
                         (add s 7)
                         (del s 7)))))
(r:drive "raw-del"
         (r:stmt-run (fn [j]
                       (let [m @{}]
                         (%put m :k (%pair 1 2))
                         (%del m :k)))))
(r:drive "raw-del-immediate"
         (r:stmt-run (fn [j]
                       (let [m @{}]
                         (%put m :k 7)
                         (%del m :k)))))
