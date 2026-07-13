(elle/epoch 12)
# tests/integration/fixtures/region-container-read-escape-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because it SIGSEGVs under
# --trace=guardfree, and `make smoke` globs tests/elle/*.lisp into one shared
# process where a segfault would take the whole harness down. Exercised by the
# guardfree subprocess pin in tests/integration/elle_scripts.rs
# (`region_container_read_escape_uaf`), #[ignore]'d RED until the over-free is fixed.
#
# WHAT IT REPRODUCES — an unsound ownership ADOPTION when a heap element is READ
# out of a LOCAL OWNED container and the read result ESCAPES.
#   `(%array-push a (list 1 2))` on a local `@[]` the ownership forest made Owned
#   emits an `AdoptRegion` moving the list into `a`'s Owned subtree (the list is
#   proven interior to `a`). But `(first a)` READS the list back out and the result
#   is RETURNED — so the list ALSO escapes. The forest never saw that escape (the
#   read result is a fresh value-flow node not linked back to the pushed list), so it
#   adopted the list anyway. `a`'s scope-exit subtree drop then frees the list while
#   the returned Value still points into it, and a later deref of the recycled page
#   faults (state-dependent — it lands once region ids recycle, so the fixture primes
#   id churn first).
#
# DISTINCT FROM `region-pop-tail-moves-out-uaf` (which is FIXED). `%pop` REMOVES the
# element, so its funnel EXTRACTS it from the container's subtree
# (`extract_owned_region`). A READ (`first`/`get`/`rest`) borrows — the element STAYS
# in the container — so the fix is not an extract; it is that escape must propagate
# through a container READ: when a read result escapes, the container's stored
# contents escape and must not be adopted. This is the general container-read-escape
# case; `%pop` was only one instance of it.
#
# THE TRIGGER IS THE RAW `%array-push`. The stdlib `push` WRAPPER is guardfree-clean:
# the container flows through the wrapper as an opaque param, so the forest does not
# adopt its contents. Hand-written raw `%array-push` inline is what emits the
# (here unsound) `AdoptRegion`; the read may be the stdlib `first` and it still
# faults, because the adoption already happened at the raw push.
#
# WHEN FIXED — escape marks the read-out-and-returned element escaping, the forest
# refuses to adopt it, and the ordinary RC path keeps it alive across the caller's
# read; this exits 0 under guardfree — un-#[ignore] the pin then.

(defn stmt-run [thunk]
  (fn [b]
    (def @i 0)
    (while (< i b)
      (thunk)
      (assign i (+ i 1)))))

# Prime: churn region ids so the freed page below is recycled onto a live region.
((stmt-run (fn []
             (let [a @[]]
               (%array-push a (list 1 2))
               (first a)))) 1000)

# Raw `%array-push` then a RETURNED raw `%first` — the read result escapes the
# container while the pushed list is adopted into it, so `a`'s drop over-frees it.
((stmt-run (fn []
             (let [a @[]]
               (%array-push a (list 1 2))
               (first a)))) 1000)

(println "region-container-read-escape-uaf: ok")
