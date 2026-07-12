(elle/epoch 12)
# tests/integration/fixtures/region-mut-container-compensation-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because a regression ABORTS the
# process (the over-free faults under --trace=guardfree, and the debug edge-table
# equivalence oracle panics), and `make smoke` globs tests/elle/*.lisp into one
# shared process where an abort takes the whole harness down. Exercised by the
# guardfree subprocess pin in tests/integration/elle_scripts.rs
# (`region_mut_container_compensation_uaf`).
#
# THE INVARIANT — the F1b container compensation releases ONLY the wrapper's
# stranded owned-param reference, never a live container.
#   A polymorphic store wrapper (`push`/`put`/`add`) reached through a value (not
#   a statically-proven type) runs its `(match (type-of coll) …)` body; the
#   mutable arm tail-calls a `-mut` funnel (`%push-array-mut`/`%put-struct-mut`/
#   `%add-set-mut`) that returns the container arg0 pass-through. The wrapper holds
#   an owned-param reference to that container which it never releases (the
#   container is return-escaping), so the region leaks 1/op. The close balances it
#   with a per-arm release in the wrapper body (`regions::compensate`,
#   `funnel_container_sites`) plus suppressing the redundant tail ReturnValue retain
#   (`lir::lower::control::call`). SAFETY: the funnel's `pass_through_retain` already
#   handed the caller one owning reference to the returned container, so releasing
#   the owned-param reference can NEVER drop the live container to zero. A misfire
#   here would free a container the caller still holds — an over-free that faults
#   under guardfree.
#
# THE CONTRAST driving the loop:
#   - a block-local accumulator DISCARDED at return (must free wholesale, cleanly);
#   - an ESCAPING accumulator RETURNED to the caller and read back (must survive —
#     the over-free witness: if the compensation freed the returned container, the
#     read-back faults or reads garbage);
#   - a NESTED pass-through wrapper (a user fn that tail-returns a `put` result),
#     driving the container through two owned-param frames.
# Id recycling across the loop makes a drifted decref land on a live region
# deterministically, not only when ids happen to collide.

# Build a mutable accumulator through the wrapper reached as a VALUE (funnel is not
# statically prunable), then RETURN it — the escaping-container case.
(defn build-array [n]
  (def @acc @[])
  (def @j 0)
  (while (%lt j n)
    (push acc {:x j})
    (assign j (%add j 1)))
  acc)

(defn build-set [n]
  (def @acc @||)
  (def @j 0)
  (while (%lt j n)
    (add acc (list j (%add j 1)))
    (assign j (%add j 1)))
  acc)

# Nested pass-through wrapper: `store` receives an owned-param container and
# tail-returns the `put` result (the container), through two frames.
(defn store [c k v]
  (put c k v))

(defn churn [reps]
  (def @c 0)
  (while (%lt c reps)
    # escaping array accumulator — build, return, read every element back
    (let [a (build-array 8)]
      (assert (= (length a) 8) "escaped array length")
      (assert (= (get (get a 0) :x) 0) "escaped array first element live")
      (assert (= (get (get a 7) :x) 7) "escaped array last element live"))
    # escaping set accumulator — build, return, read membership back
    (let [s (build-set 8)]
      (assert (= (length s) 8) "escaped set length")
      (assert (has? s (list 3 4)) "escaped set member live"))
    # nested pass-through wrapper into a fresh @struct, read the stored value back
    (let [m (store @{:x 0} :k (string "v" c))]
      (assert (= (get m :k) (string "v" c)) "nested-wrapper stored value live"))
    # block-local accumulator DISCARDED — must reclaim wholesale, no over-free
    (let [d @[]]
      (push d (list c c))
      nil)
    (assign c (%add c 1))))

# IMMUTABLE put + REASSIGN, measured through `arena/allocs` — the fresh put result
# is stored into the reassigned slot `st`, whose move consumes the result's
# ReturnValue retain. Locus B must NOT drop that retain for an immutable funnel
# (only a `-mut` pass-through), or the result is freed under the reassign — the
# `resource.lisp` struct-assoc UAF. Kept out of the churn loop above (each
# `arena/allocs` measurement is heavy); its own loop drives id recycling.
(defn reassign-churn [reps]
  (def @k 0)
  (while (%lt k reps)
    (let [pair (arena/allocs (fn []
                               (def @st {:a 0 :b 0})
                               (each i in (range 30)
                                 (assign st (put st :a i)))
                               st))]
      (assert (= (get (first pair) :a) 29) "immutable put+reassign result live"))
    (assign k (%add k 1))))

(churn 4000)
(reassign-churn 200)
(println "region-mut-container-compensation-uaf: ok")
