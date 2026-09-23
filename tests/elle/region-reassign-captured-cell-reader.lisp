(elle/epoch 12)
# audited: 2026-09-23
# A whole-value read out of a reassigned captured cell takes a counted reference.
# docs/impl/region/cells.md
#
# The trap: the cell's overwrite (`capture_store_with_rebind`) decrefs the
# displaced value unconditionally, so a local bound to the cell's whole value
# is freed under the reader unless the read took a counted reference.
#
# Two binding scopes owe it:
#   - fn-local: the cell lives in an enclosing fn and a nested closure reads it
#     through an upvalue, as in
#       (let [batch ready] (assign ready @[]) (each x in batch …))
#   - module-scope: a top-level `def @cell` read into a local.
#
# The over-freed array is the one the PRIOR call's overwrite left in the cell
# (sole-held, rc 1); the next call snapshots it into `batch`, overwrites the
# cell, then dereferences `batch` — a read of freed pages. On the plain VM the
# page is stale-but-intact (the length is still 0, so the asserts below hold);
# under `--trace=guardfree` the freed page is `PROT_NONE` and the read faults
# (SIGSEGV), the robust oracle. Pinned there by
# `region_reassign_captured_cell_reader` (tests/integration/elle_scripts/frames.rs).

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
    # `step` came out of `make-fn-local` as a value, so its result types as
    # unknown; the coerce-guard proves the %add operand without disturbing the
    # per-iteration overwrite/read cycle (the pin).
    (let [n (step)]
      (assign acc (if (%int? n) (%add acc n) acc)))
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
