(elle/epoch 12)
# audited: 2026-09-14
# A `let`-bound capture cell an assign repoints, read back across a park — the
# shape issue #1124 reports.
#
# tests/integration/fixtures/region-capture-cell-let-reassign-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because the defect it pins is an
# uncatchable SIGSEGV on plain runs once the freed pages are large enough to be
# returned rather than recycled, and `make smoke` globs tests/elle/*.lisp into
# one shared process. It is exercised by the guardfree subprocess pin in
# tests/integration/elle_scripts.rs (`region_capture_cell_let_reassign_uaf`),
# which faults deterministically at the stale deref if the defect returns.
#
# WHAT IT REPRODUCES
#   A `let`-bound `@`-mutable that is BOTH captured by a closure — so the lowerer
#   boxes it in a compiled `MakeCaptureCell` held in the binding's own slot — AND
#   reassigned. The init value's release was ROUTED through that slot:
#   `record_region_slot` recorded `region_to_slot[init_region] = the cell slot`,
#   so at the init region's decref_point the lowerer emitted `LoadLocal slot +
#   DecrefValueRegion`. `DecrefValueRegion` resolves its target with
#   `result_region_of`, which UNWRAPS a capture cell to its CURRENT content — and
#   the reassignment has repointed the cell at a later, still-live value. So the
#   release freed that live value and the displaced init leaked:
#       [guardfree] SIGSEGV — use-after-free
#           free site: DecrefValueRegion of capture-cell (runtime region N)
#           context: UpdateCapture
#
#   `def` and `var` do not reach it: `lower_define` and `lower_letrec` both skip
#   the routing for a captured-and-reassigned binding and drop the init's
#   producer reference off the value register instead. `lower_let` mints the same
#   cell into the same slot, so it owes the same two things
#   (docs/impl/region/cells.md § "Every binder that mints the cell owes the rule
#   too"). Compile-level twins, which fail without a park and without timing
#   luck: `lir::lower::tests::release::arms`'s
#   `let_bound_reassign_*_leaves_no_cell_slot_release`.
#
# THE TRAP — a park is what makes the extra release fatal rather than latent.
#   Reading the cell from the SAME fiber sees the right answer either way: the
#   freed page keeps its bytes and nothing has reclaimed it yet. A scheduler park
#   rebuilds the value's region at rc 1, so the routed release takes it to zero
#   and frees it before the reader runs. Every face below therefore reads the cell
#   across a park, from a spawned fiber. Do not "green" a regression by dropping
#   the `ev/spawn` or the suspending call inside it.
#
# THE COUNTER-FACTUAL — the wrong answer that looked right.
#   Asserting on `(length cap)` alone passes while corrupt: a recycled page
#   spells a plausible length. Each face asserts the CONTENTS the fiber read
#   back, so a stale read fails the assert even on a run that does not fault.

# ── 1. the minimal shape: a park between the capture and the read ──────────────
(let [@cap (list 1 2)]
  (assign cap (list 3 4))
  (let [w (ev/spawn (fn []
                      (ev/sleep 0.01)
                      (list (first cap) (first (rest cap)))))]
    (assert (= (ev/join w) (list 3 4))
            "a let-bound captured+reassigned cell reads its current value across a park")))
(println "let-reassign-1 ok")

# ── 2. the reported shape (#1124): a spawned fiber writes the captured buffer ───
#       `port/write` suspends, so the fiber parks with the cell's content held as
#       a `PortOp::Write` argument. The init-region release, routed through the
#       repointed cell slot, frees exactly that argument.
(let [p (port/open "/dev/null" :write)
      @buf (bytes 1 2 3 4 5 6 7 8)]
  (while (< (length buf) 65536) (assign buf (concat buf buf)))
  (let [w (ev/spawn (fn []
                      (each i in (range 0 4)
                        (port/write p buf))
                      (length buf)))]
    (assert (= (ev/join w) 65536)
            "a spawned fiber writes the captured+reassigned buffer intact")))
(println "let-reassign-2 ok")

# ── 3. the write in a closure the let body encloses, not in the body ───────────
#       The reassign is a fact about the BINDING, so the routing has to go
#       whichever scope repoints the cell (cells.md § "The reassign is a fact
#       about the BINDING, not about where the assign sits").
(let [@acc (list 0)]
  (def push (fn (x) (assign acc (list x acc))))
  (push 1)
  (push 2)
  (let [w (ev/spawn (fn []
                      (ev/sleep 0.01)
                      (first acc)))]
    (assert (= (ev/join w) 2)
            "a let-bound cell repointed from inside a closure reads back live")))
(println "let-reassign-3 ok")

(println "region-capture-cell-let-reassign-uaf: OK")
