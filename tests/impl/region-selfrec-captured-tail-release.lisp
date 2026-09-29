(elle/epoch 12)
# audited: 2026-09-29
# A self-recursive closure a sibling also captures is released by its cell, never by a tail-call deferral.
# docs/impl/selfrec.md
#
# The sidecar arms guardfree.
#
# A purely self-recursive local closure is cell-free (docs/impl/selfrec.md — the
# self-edge does not mark the binding captured), so its per-call region, stranded
# past the frame-replacing `TailCall`, is reclaimed by a tail-call deferred release
# (`stranded_self_bindings` + `tail_callee_defers_release`). But when a SIBLING captures
# the same binding it gains a forward cell: the cell holds a counted reference to
# the closure region and releases it by the cell's cascade, whose lifetime
# outlives any single tail-call activation. Marking such a cell-held binding
# stranded makes the deferred release decref its region a SECOND time — freeing it under the
# still-live cell. The next call through the sibling then tail-calls a closure
# whose region was recycled: a stale `tail_callee_release_region` deref (a
# generation panic in the interpreter, a SIGSEGV under `--trace=guardfree`).
# Only CELL-FREE self-recursive bindings are marked stranded (`needs_capture`
# excludes the sibling-captured ones).
#
# This is the std/process (and stdlib ev/run) scheduler shape: the mutually
# recursive `(defn handle-fiber-after-resume …)` group — each member
# self-recursive AND captured by its siblings — which tests/lang/process-io.lisp
# drives.

# `step` is self-recursive (tail body) AND captured by `other` → cell-held.
(defn make []
  ## Entry coerce-guard: `step` is captured by `other` (a value use), so its
  ## params take no call-site proofs.
  (defn step [n0]
    (let [n (if (%int? n0) n0 0)]
      (if (%lt n 1) 0 (step (%sub n 1)))))
  (defn other [n]
    (step n))
  other)

(let [f (make)]
  # Each call tail-calls `step` (deferral-eligible) and recurses. If `step`'s region
  # is freed by the first call's deferred release, the second call derefs a recycled page.
  (assert (= (f 5) 0) "call 1: step region already stale")
  (assert (= (f 6) 0) "call 2: step region freed by call 1's tail-release")
  (assert (= (f 7) 0) "call 3: step region freed under its capturing cell"))

(println "region-selfrec-captured-tail-release: ok")
