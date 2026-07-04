(elle/epoch 12)
# tests/elle/region-reassign-return-park-uaf.lisp
#
# Regression guard for the io/scheduler cross-fiber UAF (hand-off.md headline;
# the `chan/select`-timeout family). The fault is NOT in the io layer — it is a
# potential region-solver double-release of a RETURNED fn-local reassigned
# mutable, made fatal by a scheduler park.
#
# THE SHAPE. A function whose result is a fn-local reassigned mutable that is
# read at the tail (returned):
#     (def @result nil) ... (assign result V) ... (break) ... result
# The reassign gate refuses the 1-slot-container model for a *returned* mutable
# (it gates on not-returned, via `EscapeInfo`'s return facet), so the callee
# emits its ordinary value-based decref of the result's region at scope exit.
# The always-mint return convention balances that against the caller: every
# `Return` mints a fresh owning reference (`lower_return`'s `IncrefValueRegion`)
# which the caller's `DecrefValueRegion` at the call site releases — so the
# callee decref and the caller release are one reference each, never two on one.
# The hazard this guards: were the mint dropped, or the gate to suppress the
# callee decref on a returned mutable, the result would be released twice — one
# reference, two decrefs. See `src/hir/regions/analyze.rs` (the fn-local
# reassign gate) and docs/impl/region-bindings.md "Reassigned mutable bindings
# are 1-slot containers".
#
# WHY IT HID. The double-release is LATENT: with no park the returned value
# transiently has rc>=2 (alloc + cell), so the extra decref only drops it to
# rc 1 and the program reads stale-but-intact bytes (a false green in plain
# `--jit=off`). A scheduler PARK (here `ev/sleep`, in the corpus `chan/select`)
# built the value AFTER the resume with rc 1, so the callee's extra decref
# frees the live result before the caller reads it -> UAF.
#
# WHY THE CORPUS, NOT THE LIB SUITE, CAUGHT IT. The bug only fires when the
# callee has NO direct (statically-resolved) call site in its compilation unit
# — only then does the solver skip `try_inline_call`, whose re-walk in the
# caller's escaping context happens to suppress the return region. Every
# suspending stdlib function (`chan/select`, `chan/wait-ready`, …) is called
# only cross-unit from user code, so all of them hit it. We reproduce that
# here in ONE unit by calling `worker` OPAQUELY (through a stored value) so the
# caller cannot classify it and the inline path does not run.
#
# Manifestation: under `--trace=guardfree` the freed result page faults at the
# resumed read; in plain mode the assert below catches the wrong value.

(defn worker [n0]
  (def @result nil)
  (def @n n0)
  (forever
    (if (<= n 0)
      (begin
        (assign result [:done 1 2 3])  ## built AFTER the prior iteration's park
        (break))
      (begin
        (ev/sleep 0.001)  ## scheduler park; resumes into the next iteration
        (assign n (- n 1)))))
  result)

## Opaque call: `worker` is fetched from a container, so the caller sees an
## unknown callee — no static classification, no inline re-walk.
(let [tbl @[worker]
      f (get tbl 0)
      got (f 1)]
  (def junk @[])
  (each i in (range 0 64)
    (push junk [i i i]))
  (assert (= got [:done 1 2 3])
          (string "returned reassigned-mutable result corrupted across park: "
                  got)))

(print :ok)
