(elle/epoch 12)
# Counterfactual for the tail-call CLOSURE-CALLEE region leak.
#
# This is the dual of region-tailcall-arg-transfer.lisp. That file fixed the
# tail-call *argument* leak (an owned arg's dead post-`TailCall` decref IS its
# ownership transfer to the callee). This file pins the leak that remained: the
# tail call's *callee*, when it is an owned, heap-allocated CLOSURE value bound
# to a local, leaks the closure's region — one region per call.
#
# Mechanism (re-derived 2026-06-16, independent of any prior note):
#   - The closure `(fn …)` is an ordinary heap allocation; the solver gives it a
#     region and a `decref_point` AT the tail-call node (its last use is the
#     call). Verified via `--dump=regions`: the closure region appears in the
#     decref-points list, dying at the tail-call HirId.
#   - The lowerer (`src/lir/lower/control/call.rs`, `lower_call` `is_tail` arm)
#     emits the closure region's `DecrefValueRegion` via the enclosing
#     `lower_expr`'s TRAILING `emit_decrefs_for`, AFTER the `TailCall`. A tail
#     call replaces the frame, so that trailing block is dead code and never
#     runs. The closure region leaks.
#   - It is TAIL-SPECIFIC: a non-tail call of the same local closure releases it
#     normally (the `non-tail-*` tiers below are bounded today).
#   - The naive fix ("free the closure region just before `TailCall`") is a
#     use-after-free for a CAPTURING closure: `populate_env` copies the
#     captured env UNCOUNTED, and a capture records NO `cross_region_ref` edge
#     (the captured value is kept alive ONLY by the closure region). Freeing the
#     closure region would cascade-free the captures out from under the running
#     callee. A correct fix must keep the closure region alive for the callee's
#     lifetime (adopt-and-release at the new frame's teardown, or incref the
#     copied captures). The CORRECTNESS asserts below guard against a fix that
#     frees too early — they must keep passing.
#
# Measured in REGIONS (`arena/region-count`), not objects: a leaked closure
# carries its inline env payload, so object-count (`arena/count`) can stay
# bounded while regions grow. The teardown-residue oracle is a region count, so
# this is the class that moves it.
#
# Counterfactual status: the BOUNDEDNESS asserts are RED until the closure-callee
# release lands; the CORRECTNESS asserts are GREEN now and must stay green.

(defn region-delta [f iters]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i iters)
    (f i)
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

# ── callees: a local closure invoked in tail vs non-tail position ──

# (1) non-capturing closure, TAIL-called → leaks the closure region today.
(defn tail-noncap [k]
  (let [f (fn [] 7)]
    (f)))

# (2) non-capturing closure, NON-tail (bind result, return it) → bounded today.
(defn nontail-noncap [k]
  (let [f (fn [] 7)]
    (let [r (f)]
      r)))

# (3) capturing closure (captures heap struct h), TAIL-called → leaks the
#     closure region AND the captured struct's region today. The callee reads
#     the captured value, so a too-early free of the closure region is a UAF.
(defn tail-cap [k]
  (let [h {:k k}]
    (let [f (fn [] (get h :k))]
      (f))))

# (4) capturing closure RETURNING the captured heap value, TAIL-called → the
#     returned capture must survive (no UAF) and not leak.
(defn tail-cap-ret [k]
  (let [h {:k k :v (string "s-" k)}]
    (let [f (fn [] h)]
      (f))))

# ── Correctness (guards a too-early-free of the adopted closure; GREEN) ──
# These must ALL pass: the adoption must not free a closure region whose captured
# values the callee still reads or returns. The capturing cases are the hazard —
# their captures live in the closure's region pages (copied UNCOUNTED by
# `populate_env`) and a returned capture must outlive the activation.
(assert (= (tail-noncap 1) 7) "tail noncap closure returns 7")
(assert (= (nontail-noncap 1) 7) "nontail noncap closure returns 7")
(assert (= (tail-cap 42) 42) "tail capturing closure reads its capture")
(assert (= (get (tail-cap-ret 7) :k) 7)
        "tail capturing closure returns its capture intact")

# ── Boundedness: the closure-callee leak is reclaimed ──
# `tail-noncap` is the pure case — a per-call local closure tail-called, no
# captures. Its region (a closure template + instance) is now adopted by the new
# activation and released on completion, matching the non-tail `nontail-noncap`
# control. Both must stay bounded (rate ~0); they were the RED counterfactual
# before the adopt-and-release landed.
(let [d1 (region-delta tail-noncap 200)
      d2 (region-delta nontail-noncap 200)]
  (assert (%lt d2 20)
          (concat "nontail-noncap (control) leak: delta=" (number->string d2)))
  (assert (%lt d1 20)
          (concat "tail-noncap closure-callee leak: delta=" (number->string d1))))

# ── Residual (documented; NOT the closure-callee leak) ──
# `tail-cap`/`tail-cap-ret` still grow — but that residue is NOT the closure-callee
# leak (the closure region IS now adopted and freed). It is the CAPTURED LOCAL `h`,
# whose own alloc-reference decref is also stranded past the frame-replacing tail
# call. Measured per iteration:
#   - tail-cap `{:k k}`                   → ~1 region/iter: the captured struct `h`.
#   - tail-cap-ret `{:k k :v (string …)}` → ~2 regions/iter: the captured struct `h`
#     PLUS the string it holds in `:v` (that string region is pinned by the leaked
#     `h`, freed only when `h` is). The `(string …)` call itself no longer leaks an
#     extra region per heap argument — that was a separate `RegionEffect::Mixed`
#     mis-declaration on the `string` primitive, since fixed (`string` is now
#     `Fresh`; pinned by tests/elle/region-string-concat-leak.lisp), which is why
#     this residue is 2/iter, not the 3/iter it measured before that fix.
# This captured-local class is distinct from the callee closure's leak and is closed
# by the forest's capture-in-tail owned-subtree reclamation, not here. A SELF-recursive
# captured closure (`fold`/`map`/`filter`'s `go`) additionally leaks via a cell↔closure
# reference CYCLE that RC cannot reclaim (the mutable-cycle incompleteness — closed only
# by owned-region reclamation). Asserted bounded below ONLY loosely (shrink-only canary:
# the residue must not GROW past its measured rate), so the forest fix flips these
# tighter (toward 0).
(let [d3 (region-delta tail-cap 200)
      d4 (region-delta tail-cap-ret 200)]
  (assert (%lt d3 300)
          (concat "tail-cap residue grew past captured-local rate (~1/iter): delta="
                  (number->string d3)))
  (assert (%lt d4 500)
          (concat "tail-cap-ret residue grew past captured-local rate (~2/iter): delta="
                  (number->string d4))))
