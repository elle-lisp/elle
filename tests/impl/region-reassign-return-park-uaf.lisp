(elle/epoch 12)
# audited: 2026-09-29
# A returned fn-local reassigned mutable is released once by the callee and once by the caller, across a park.
# docs/impl/region/bindings.md
#
# The hazard is a region-solver double-release of a RETURNED fn-local reassigned
# mutable, made fatal by a scheduler park. It shows up in the io layer (the
# `chan/select` timeout family) but is not the io layer's.
#
# THE SHAPE. A function whose result is a fn-local reassigned mutable that is
# read at the tail (returned):
#     (def @result nil) ... (assign result V) ... (break) ... result
# The callee holds exactly ONE reference of its own and releases it exactly once:
# the 1-slot container takes a counted reference at the store and drops it at its
# scope demise, which the lowerer emits at the `Return` node — after that node's
# mint. The always-mint return convention supplies the caller's side: every
# `Return` mints a fresh owning reference (`lower_return`'s `IncrefValueRegion`)
# which the caller's `DecrefValueRegion` at the call site releases. So callee and
# caller hold one reference each, never two on one. The hazard this guards: were
# the mint dropped, or a second callee release emitted against the one callee
# reference, the result would be released twice. See
# `src/hir/region/infer/analyze/reassign.rs` (the fn-local reassign gate) and
# docs/impl/region/bindings.md.
#
# THE TRAP. The double-release is LATENT: with no park the returned value
# transiently has rc>=2 (alloc + cell), so an extra decref only drops it to rc 1
# and the program reads stale-but-intact bytes. A scheduler PARK (here
# `ev/sleep`, in the stdlib `chan/select`) builds the value AFTER the resume
# with rc 1, so an extra callee decref frees the live result before the caller
# reads it.
#
# WHY AN OPAQUE CALL. The double-release needs a callee with NO direct
# (statically-resolved) call site in its compilation unit — only then does the
# solver skip `try_inline_call`, whose re-walk in the caller's escaping context
# suppresses the return region. Every suspending stdlib function (`chan/select`,
# `chan/wait-ready`, …) is called only cross-unit from user code, so all of them
# have this shape. The file reproduces that in ONE unit by calling `worker`
# OPAQUELY (through a stored value), so the caller cannot classify it and the
# inline path does not run.
#
# Manifestation: under `--trace=guardfree` the freed result page faults at the
# resumed read; on a plain run the assert below catches the wrong value.

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
