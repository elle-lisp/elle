(elle/epoch 12)
# tests/integration/fixtures/region-pop-tail-moves-out-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because a regression ABORTS the
# process (the over-free faults under --trace=guardfree, and the debug edge-table
# equivalence oracle / generation stamp panics at the drifted free), and
# `make smoke` globs tests/elle/*.lisp into one shared process where an abort takes
# the whole harness down. Exercised by the guardfree subprocess pin in
# tests/integration/elle_scripts.rs (`region_pop_tail_moves_out_uaf`).
#
# A `moves_out` REMOVE (`%pop`) returns a HEAP element pushed into a LOCAL OWNED
# container via a funnel — two defects, one crash and one leak.
#
#   THE CRASH (over-free). `(%array-push a (list 1 2))` on a local `@[]` the
#   ownership forest made Owned emits an `AdoptRegion` that moves the list into `a`'s
#   Owned subtree (RC frozen — `incref`/`decref` are then inert). `%pop` moving the
#   list back OUT must EXTRACT it from that subtree: un-record the
#   `region(a) → region(list)` edge
#   and move the list `Owned → Counted(1)` (`extract_owned_region`), the caller's one
#   owning reference. Without the extract the list stays interior, so `a`'s scope-exit
#   subtree drop frees it while the returned Value still points into it, and a later
#   deref of the recycled page faults (a stale-region-deref). NON-tail statement
#   position discards the result, so the premature free is silent there (`raw-pop`
#   reads 0); returning it (tail) surfaces the fault.
#
#   THE LEAK (over-keep, tail only). The native-tail path emits a ReturnValue
#   `IncrefValueRegion` over the popped element, but a moves-out element already
#   carries its one caller reference (the in-body escape retain, or the extract's
#   `Counted(1)`), so that retain is redundant and tail `%pop` leaks 1 region/op. It
#   is suppressed for a moves_out ∩ PassThrough site (`moves_out_release_sites`).
#
# The fault is state-dependent — it lands only once region ids recycle onto the
# freed one — so a short loop looks clean. The PRIMING loop churns ids first; the raw
# tail-pop loop then faults deterministically. A rewrite must not be "verified" on a
# short run.

(defn stmt-run [thunk]
  (fn [b]
    (def @i 0)
    (while (< i b)
      (thunk)
      (assign i (+ i 1)))))

# Prime: churn region ids so the freed page below is recycled onto a live region.
((stmt-run (fn []
             (let [a @[]]
               (push a (list 1 2))
               (pop a)))) 1000)

# The raw `%array-push` + tail `%pop` — over-frees the popped list's region.
((stmt-run (fn []
             (let [a @[]]
               (%array-push a (list 1 2))
               (%pop a)))) 1000)

(println "region-pop-tail-moves-out-uaf: ok")
