(elle/epoch 12)
# A whole-value binding read out of a REASSIGNED CAPTURED CELL must take a
# counted reference (docs/impl/region/bindings.md "Captured reassigned cells").
#
# A captured, reassigned mutable binding is a 1-slot container whose overwrite
# (`UpdateCapture` / `capture_store_with_rebind`) decrefs the displaced prior
# UNCONDITIONALLY. A local that binds the cell's WHOLE value is an alias of that
# prior; without a counted reference of its own the overwrite frees the value out
# from under it — the captured-alias use-after-free. The reader takes Rule 5's
# "new reference" pass-through (an `IncrefValueRegion` at the read, the balancing
# `DecrefValueRegion` at the reader's last use).
#
# The obligation holds for BOTH binding scopes, and neither was pinned before:
#   - fn-local: the cell lives in an enclosing fn and is read through an UPVALUE
#     by a nested closure. This is the std/process scheduler's `ready`
#     double-buffer — `sched-run`'s
#       (let [batch ready] (assign ready @[]) (each pid in batch (run-one pid)))
#     where `ready` is a `make-scheduler` local; a regression there SIGSEGVs the
#     whole tests/elle/process-io.lisp suite.
#   - module-scope: a top-level `def @cell` read into a local.
#
# The over-freed array is the one the PRIOR call's overwrite left in the cell
# (sole-held, rc 1); the next call snapshots it into `batch`, overwrites the
# cell, then dereferences `batch` — a read of freed pages. On the plain VM the
# page is stale-but-intact (the length is still 0, so the asserts below hold);
# under `--trace=guardfree` the freed page is `PROT_NONE` and the read faults
# (SIGSEGV), the robust oracle. Pinned there by
# `region_reassign_captured_cell_reader` (tests/integration/elle_scripts.rs).

# ── (a) fn-local: reassigned cell read through an upvalue ──────────────
(defn make-fn-local []
  (def @ready @[])
  (def @touch (fn [] ready))  # a sibling captures `ready` → it is a cell
  (fn []
    (let [batch ready]  # whole-value upvalue read of the reassigned cell
      (assign ready @[])  # overwrite frees batch's array if uncounted
      (length batch))))  # deref the (freed?) array header

(let [step (make-fn-local)]
  (def @acc 0)
  (var i 0)
  (while (%lt i 300)  # recycle physical ids so a stale read lands on reuse
    (assign acc (%add acc (step)))
    (assign i (%add i 1)))
  (assert (= acc 0) "fn-local reassigned-cell reader was freed by the overwrite"))

# ── (b) module-scope: reassigned top-level cell read into a local ──────
(def @mcell @[])
(def @mtouch (fn [] mcell))
(defn mstep []
  (let [batch mcell]
    (assign mcell @[])
    (length batch)))

(def @macc 0)
(var j 0)
(while (%lt j 300)
  (assign macc (%add macc (mstep)))
  (assign j (%add j 1)))
(assert (= macc 0)
        "module-scope reassigned-cell reader was freed by the overwrite")

(println "region-reassign-captured-cell-reader: ok")
