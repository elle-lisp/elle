(elle/epoch 12)
# Counterfactual for the call-result-arg clique UAF
# (docs/impl/region/effects.md "Native region effects" — the incref side of
# "What the solver derives").
#
# A Mixed/Unknown call's arg-clique incref was a compile-time
# `IncrefRegion` keyed by the argument's STATIC region slot. A
# call-RESULT region is a marker: no alloc opcode ever records a runtime
# mint under its slot, so that IncrefRegion resolved to nothing — a
# silent no-op. The balancing decref is NOT a no-op: when the store
# target's region frees, its content cascade decrefs the stored value's
# real region. Net: a Mixed native that stores a call-result-region
# argument frees that argument's region out from under live readers.
#
# Shape: `[:k :reason]` lowers to an array-constructor CALL, so its
# region is a call-result marker. fiber/cancel and fiber/abort (Mixed)
# store it into fiber.signal uncounted and return it pass-through. When
# f's region frees at f's last use, the cascade finds fiber.signal's
# array and decrefs its region — with the no-op incref that stole v's
# reference (RED at the pre-fix HEAD: stale-region/tag-mismatch panic in
# debug builds; guardfree SIGSEGV). The fiber/status read between the
# store and v's read allocates into the freed page so the stale read
# manifests deterministically.
#
# GREEN requires the clique incref for a call-result-region argument to
# be VALUE-based — load the arg's binding slot and retain the region the
# value actually lives in, the exact mirror of the call-result decref
# path (src/lir/lower/regionemit.rs `emit_increfs_for`). This applies
# exactly at hard-edge sites: native calls with declared uncounted-store
# effects (Stores/Mixed/Unknown — docs/impl/region/effects.md "Hard edges"). Opaque
# user-fn sites keep the no-op slot path, pinned by
# region-userfn-clique-callresult-noleak.lisp.

(let* [f (fiber/new (fn [] "never-reached") 1)
       v (fiber/cancel f [:k :reason])
       s (fiber/status f)]
  (assert (= s :error) "cancel: fiber status is error")
  (assert (= v [:k :reason]) "cancel: stored+returned arg survives the fiber"))

(let* [f (fiber/new (fn [] "never-reached") 1)
       v (fiber/abort f [:k :reason])
       s (fiber/status f)]
  (assert (= s :error) "abort: fiber status is error")
  (assert (= v [:k :reason]) "abort: stored+returned arg survives the fiber"))

(println "region-native-clique-callresult-uaf: ok")
