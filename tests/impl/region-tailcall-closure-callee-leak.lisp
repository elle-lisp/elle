(elle/epoch 13)
# audited: 2026-09-29
# A local closure called in tail position is released with the frame it replaces, and its captures stay readable.
# docs/impl/region/relocate.md
#
# region-tailcall-arg-transfer.lisp covers the tail call's ARGUMENTS. This file
# covers its CALLEE: an owned, heap-allocated CLOSURE value bound to a local.
# The solver gives the closure a region whose `decref_point` is the tail-call
# node, and a release emitted after the `TailCall` never runs, because the
# call replaces the frame. So the new activation takes the release over (the
# `TailCall`'s `defer_callee_release`) and runs it at its own completion.
#
# The counter-factuals:
#   - A release left after the `TailCall` leaks one region per call.
#   - A release moved just before the `TailCall` frees a CAPTURING closure's
#     env under the running callee. `populate_env` copies the captured env
#     UNCOUNTED, and a capture records no `cross_region_ref` edge, so the
#     closure region is the only thing keeping the captures alive.
#
# Measured in REGIONS (`arena/region-count`), not objects: a leaked closure
# carries its inline env payload, so the object count can stay flat while
# regions grow.

(defn region-delta [f iters]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i iters)
    (f i)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

# ── callees: a local closure invoked in tail vs non-tail position ──

# (1) non-capturing closure, TAIL-called.
(defn tail-noncap [k]
  (let [f (fn [] 7)]
    (f)))

# (2) non-capturing closure, NON-tail (bind result, return it): the control.
(defn nontail-noncap [k]
  (let [f (fn [] 7)]
    (let [r (f)]
      r)))

# (3) capturing closure (captures heap struct h), TAIL-called. The callee reads
#     the captured value, so a too-early free of the closure region is a UAF.
(defn tail-cap [k]
  (let [h {:k k}]
    (let [f (fn [] (get h :k))]
      (f))))

# (4) capturing closure RETURNING the captured heap value, TAIL-called: the
#     returned capture must survive and must not leak.
(defn tail-cap-ret [k]
  (let [h {:k k :v (string "s-" k)}]
    (let [f (fn [] h)]
      (f))))

# ── Correctness: the closure is not freed under its callee ──
# The capturing cases are the hazard: their captures live in the closure's
# region pages, and a returned capture must outlive the activation.
(assert (= (tail-noncap 1) 7) "tail noncap closure returns 7")
(assert (= (nontail-noncap 1) 7) "nontail noncap closure returns 7")
(assert (= (tail-cap 42) 42) "tail capturing closure reads its capture")
(assert (= (get (tail-cap-ret 7) :k) 7)
        "tail capturing closure returns its capture intact")

# ── Boundedness: every callee shape reclaims its regions ──
# Each shape allocates one to three regions per call, so 200 calls that
# strand any of them grow the count by 200 or more.
(let [d1 (region-delta tail-noncap 200)
      d2 (region-delta nontail-noncap 200)
      d3 (region-delta tail-cap 200)
      d4 (region-delta tail-cap-ret 200)]
  (assert (%lt d2 20)
          (concat "nontail-noncap (control) leak: delta=" (number->string d2)))
  (assert (%lt d1 20)
          (concat "tail-noncap closure-callee leak: delta=" (number->string d1)))
  (assert (%lt d3 20)
          (concat "tail-cap strands its closure or its capture: delta="
                  (number->string d3)))
  (assert (%lt d4 20)
          (concat "tail-cap-ret strands its closure or its capture: delta="
                  (number->string d4))))
