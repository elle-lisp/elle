(elle/epoch 12)
# A SELF-RECURSIVE closure that is ALSO captured by a sibling must not have its
# region released by a tail-call adopt — the capturing cell owns that release.
#
# A purely self-recursive local closure is cell-free (docs/impl/selfrec.md — the
# self-edge does not mark the binding captured), so its per-call region, stranded
# past the frame-replacing `TailCall`, is reclaimed by a tail-call adopt
# (`stranded_self_bindings` + `tail_callee_adopts`). But when a SIBLING captures
# the same binding it gains a forward cell: the cell holds a counted reference to
# the closure region and releases it by the cell's cascade, whose lifetime
# outlives any single tail-call activation. Marking such a cell-held binding
# stranded makes the adopt decref its region a SECOND time — freeing it under the
# still-live cell. The next call through the sibling then tail-calls a closure
# whose region was recycled: a stale `tail_callee_adopt_region` deref (a
# generation panic on the plain VM, a SIGSEGV under `--trace=guardfree`).
#
# This is the std/process (and stdlib ev/run) scheduler shape: the mutually
# recursive `(defn handle-fiber-after-resume …)` group — each member
# self-recursive AND captured by its siblings — whose regression SIGSEGVs
# tests/elle/process-io.lisp. The fix marks only CELL-FREE self-recursive
# bindings stranded (`needs_capture` excludes the sibling-captured ones).
# Pinned under the UAF oracle by `region_selfrec_captured_tail_adopt`
# (tests/integration/elle_scripts.rs).

# `step` is self-recursive (tail body) AND captured by `other` → cell-held.
(defn make []
  (defn step [n]
    (if (%lt n 1) 0 (step (%sub n 1))))
  (defn other [n]
    (step n))
  other)

(let [f (make)]
  # Each call tail-calls `step` (adopt-eligible) and recurses. If `step`'s region
  # is freed by the first call's adopt, the second call derefs a recycled page.
  (assert (= (f 5) 0) "call 1: step region already stale")
  (assert (= (f 6) 0) "call 2: step region freed by call 1's tail-adopt")
  (assert (= (f 7) 0) "call 3: step region freed under its capturing cell"))

(println "region-selfrec-captured-tail-adopt: ok")
