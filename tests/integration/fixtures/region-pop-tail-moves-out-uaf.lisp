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
# RED — a `moves_out` REMOVE (`%pop`) in TAIL position over-frees the removed HEAP
# element when that element was stored through a push FUNNEL.
#   `(%array-push a (list 1 2))` records the outgoing edge `region(a) → region(list)`
#   and increfs the list's region; `%pop` moves the list back out (its body takes the
#   pass-through retain and un-records the edge). In NON-tail position this balances —
#   `raw-pop` (the `oracle.lisp` control, a `%pop` in a while-statement) reads 0. In
#   TAIL position the native-tail path additionally emits a ReturnValue
#   `IncrefValueRegion` over the popped element (the retain a fresh/borrow result
#   needs), which double-counts against the funnel's own accounting: the list's region
#   is freed while a live reference to it remains, and a later deref of the recycled
#   page faults (a stale-region-deref / generation mismatch).
#
# The fault is state-dependent — it lands only once region ids recycle onto the
# freed one — so a short loop looks clean. The PRIMING loop churns ids first; the raw
# tail-pop loop then faults deterministically. A rewrite must not be "verified" on a
# short run.
#
# The close is `pop`'s remaining Stage-4 work: the moved-out element's retain
# accounting must balance in tail position exactly as it does in statement position
# (the `%put-*` container half is already balanced by the container compensation —
# `region_mut_container_compensation_uaf`). Un-ignore this pin when that lands.

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
