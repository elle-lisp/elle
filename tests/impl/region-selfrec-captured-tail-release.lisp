(elle/epoch 18)
# audited: 2026-11-19
# A self-recursive closure a sibling also captures is released by its capture, never by a tail-call deferral.
# docs/impl/selfrec.md
#
# The sidecar arms guardfree.
#
# A purely self-recursive local closure is cell-free: its self-edge does not mark
# the binding captured (docs/impl/selfrec.md). Its per-call region, stranded past
# the frame-replacing `TailCall`, is reclaimed by a tail-call deferred release
# (`stranded_self_bindings` + `tail_callee_defers_release`). But a SIBLING that
# captures the same binding holds a counted reference to the closure region, and
# may call it after any one tail call to it completes. Marking such a captured
# binding stranded makes the deferred release decref its region a SECOND time,
# freeing it under the still-live capture. The next call through the sibling then
# tail-calls a closure whose region was recycled: a stale
# `tail_callee_release_region` deref (a generation panic in the interpreter, a
# SIGSEGV under `--trace=guardfree`). Only a binding no closure captures is marked
# stranded.
#
# This file pins the by-value shape: `other` captures the def `step` in its own
# environment, with no cell. The scheduler's `handle-fiber-after-resume` group has
# the cell shape, a `letrec` whose members their siblings capture, and
# tests/lang/process-io.lisp drives it.

# `step` is self-recursive (tail body) AND captured by `other` → not stranded.
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
  (assert (= (f 7) 0) "call 3: step region freed under its capture"))

(println "region-selfrec-captured-tail-release: ok")
