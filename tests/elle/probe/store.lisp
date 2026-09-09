(elle/epoch 12)
# audited: 2026-09-08
# The physical-id dimension no other gauge shows, and the remove/rebind half of the mutable-store funnel.
#
# docs/impl/region/diagnostics.md
# ── The physical-id dimension ─────────────────────────────────────────
# What a CALL costs in physical region ids, the dimension every probe above is
# blind to. A native call mints a physical region for its result before the
# callee runs, because the callee may allocate the result into it; a callee that
# returns an immediate, or a value borrowed from an argument, allocates nothing
# into that id. It never becomes a live region, so no teardown can return it, and
# it holds no object, no page, no bytes and no reference count for any other
# gauge to see (docs/impl/region/model.md § "Physical id recycling"). What it
# costs is resident: the region table is a `Vec` indexed by physical id, so the
# largest id ever made live sets its length.
#
# Both exits of the id lifecycle are gauged here, and the pair must stay
# together — the recycle admits an id only where the mint never materialized it,
# so an id that DID materialize has to reach the free list by its teardown
# instead, and a probe of one exit alone cannot tell a working recycle from one
# that hands the same id back twice. `id-immediate-result` and `id-const-compare`
# are results a native never allocates at all; `id-borrowed-element` and
# `id-borrowed-index` are results borrowed out of an argument; `id-fresh-result`
# is the materializing control, whose id comes back by the ordinary teardown.
#
# These are CLOSED controls (undeclared, like `rest-array-copy`), so a regression
# to open trips the completeness gate loudly. Read them against the id
# discriminator above: an id gauge that cannot move reads 0 for all five.
(def id-hold [1 2 3])
(println "── folded suite: physical-id recycling ──")
(pin (measure-core "id-const-compare" (stmt-run (fn [] (< 1 2))) ids-gauge 100 6
                   60 0.4 0.5) 0)
(pin (measure-core "id-immediate-result" (stmt-run (fn [] (length id-hold)))
                   ids-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "id-borrowed-index" (stmt-run (fn [] (get id-hold 0)))
                   ids-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "id-borrowed-element" (stmt-run (fn [] (first (pair 1 2))))
                   ids-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "id-fresh-result" (stmt-run (fn [] (pair 1 2))) ids-gauge 100
                   6 60 0.4 0.5) 0)

# ── The mutable-store funnel — remove/rebind half ─────────────────────
# The store half (push/put/add) is pinned above (push-churn/struct-put/set-array/…);
# these pin the REMOVE and REBIND half of the same seam (docs/impl/region/ownership.md
# § "The outgoing edge table"; src/value/arena/mutate.rs). The funnel SEAM is
# complete-by-construction — every remove co-locates its RC decref with the outgoing
# un-record, the raw accessors are private (an uncounted store is a compile error), and
# a debug equivalence oracle asserts the recorded table matches a content scan at every
# free. These pins read the seam THROUGH the surface that reaches it, and split cleanly:
#
#   `%pop` — the remove funnel balances (rate 0): a box store+rebind and
#   `%pop`'s `moves_out` native each reclaim their cross-region member, so `raw-pop` is
#   the reclaiming CONTROL (the peer of push-slot-source/put-slot-source) proving the
#   remove funnel sound, and a wrapper that leaks over it is the wrapper's leak. It is a
#   DIRECT while-statement, not a thunk: the popped value is discarded as a statement, so
#   it isolates the remove funnel's own reclamation from the return convention and from
#   the ownership forest's handling of a value pushed into a LOCAL then popped OUT and
#   RETURNED. `%pop` is a native call whose result is a distinct `call_result` region
#   with its own `DecrefValueRegion`, which balances the `moves_out` retain
#   (`pop_with_decref`) that hands the element back.
#
#   F1b remove-wrapper — the stdlib `pop`/`del` `(match (type-of coll)
#   …)` dispatch wrapper strands the container arg + fresh result on the arms the
#   textually-last arm does not reach, exactly as the STORE wrappers (put/push/set) do.
#   `pop` leaks (3): the leak is the multi-arm wrapper. Closes by the SAME mechanism as
#   the store half — per-arm compensation of the container+result, or dispatch prune on a
#   statically-typed scrutinee.
#
#   The RAW remove funnel reclaims too (`raw-del`/`raw-del-immediate` = 0): `%del`'s
#   in-place @struct/@set remove decrefs the removed member and its `-mut` pass-through
#   result carries exactly one return mint. These two are the CLOSED raw-funnel controls
#   for the remove half, the peers of `raw-pop`/`put-slot-source`. Their probe shape is
#   deliberately a two-statement body whose tail is the funnel call — the ANF-named tail
#   call whose result a `Return` mint covers (docs/impl/region/mechanism.md § "The return
#   mint is emitted exactly once") — so a second, unbalanced retain there reads here as a
#   whole stranded container plus the member it holds.
(println "── folded suite: mutable-store funnel (remove/rebind half) ──")
(pin (measure-core "box-rebind"
                   (stmt-run (fn []
                               (let [b (box (list 1 2))]
                                 (rebox b (list 3 4))))) count-gauge 100 6 60
                   0.4 0.5) 0)
# F1b — the stdlib `add` `(match (type-of coll) …)` dispatch
# wrapper reclaims its owned @set container AND its stored heap member (rate 0): the
# `:@set` arm's `%add-set-mut` returns the container pass-through, and the wrapper's
# per-arm container release (`regions::compensate`, `funnel_container_sites`) frees
# the stranded owned-param reference, cascading the stored list through the outgoing
# edge table. A CLOSED control beside the reclaiming raw funnel `set-add-slot-source`
# — RED if the container compensation regresses.
(pin (measure-core "set-add"
                   (stmt-run (fn []
                               (let [s @||]
                                 (add s (list 1 2))))) count-gauge 100 6 60 0.4
                   0.5) 0)
(pin (measure-core "raw-pop"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (def @j 0)
                     (while (%lt j b)
                       (let [a @[]]
                         (%array-push a (%pair 1 2))
                         (%pop a))
                       (assign j (%add j 1)))) count-gauge 100 6 60 0.4 0.5) 0)
# The stdlib `pop` REMOVE-of-ELEMENT wrapper reclaims (rate 0). Its `:@array`/
# `:@string`/`:@bytes` arms route to the monomorphic moves-out funnels
# `%pop`/`%pop-string`/`%pop-bytes`; the container compensation frees the wrapper's
# stranded owned-param container per-arm (recorded for a moves-out funnel even though
# it returns the ELEMENT, not the container), and the moved-out @array element's
# redundant tail ReturnValue retain is suppressed (`moves_out_release_sites`) — so
# both halves of the earlier leak close.
(pin (measure-core "pop-wrapper"
                   (stmt-run (fn []
                               (let [a @[]]
                                 (push a (list 1 2))
                                 (pop a)))) count-gauge 100 6 60 0.4 0.5) 0)
# The stdlib `del` REMOVE wrapper reclaims (rate 0), the remove-half peer of the
# store wrappers: its `:@struct`/`:@set` arms route to the `-mut` remove funnels
# (`%del-struct-mut`/`%del-set-mut`) that return the container pass-through, and the
# wrapper's container compensation frees the stranded owned-param reference — a
# CLOSED control beside the reclaiming raw funnel `put-slot-source`.
(pin (measure-core "del-wrapper"
                   (stmt-run (fn []
                               (let [m @{}]
                                 (put m :k (list 1 2))
                                 (del m :k)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "set-del-wrapper"
                   (stmt-run (fn []
                               (let [s @||]
                                 (add s 7)
                                 (del s 7)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "raw-del"
                   (stmt-run (fn []
                               (let [m @{}]
                                 (%put m :k (%pair 1 2))
                                 (%del m :k)))) count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "raw-del-immediate"
                   (stmt-run (fn []
                               (let [m @{}]
                                 (%put m :k 7)
                                 (%del m :k)))) count-gauge 100 6 60 0.4 0.5) 0)
