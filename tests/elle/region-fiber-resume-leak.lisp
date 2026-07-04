(elle/epoch 12)
# Counterfactual for the `fiber/resume` carrier-region leak (oracle.lisp's
# `fiber-resume` probe, focused).
#
# `prim_fiber_resume` returns the fiber *argument* (the carrier) as its
# SIG_RESUME signal value, so `dispatch_native_call` — which cannot tell a
# signal payload from a real result — applied the `NativeCallResult`
# pass-through incref to the fiber's OWN region, expecting the caller's
# `DecrefValueRegion` to balance it. But the resume handler REPLACES the
# carrier with the child's actual result before pushing it, so the caller's
# decref targets the RESULT's region, never the carrier's. The carrier retain
# was left dangling: a fiber resumed to completion (:dead) and dropped stayed
# at rc=1 forever, dragging its closure instance + template region with it
# (~3 objects/iteration). Rule 8: nothing leaks.
#
# The fix releases that carrier pass-through exactly on the :dead path (a
# fiber that SUSPENDED keeps it — that retain is the scheduler's liveness
# hold between pumps). Releasing it exposed a previously-masked defect:
# `fiber/parent` dereferenced the cached `parent_value` Value, which points at
# the parent's `HeapObject::Fiber` in a region the region model reclaims at the
# parent's own decref_point — a use-after-free once the parent is gone (the
# leaked carrier used to keep every parent alive forever). `fiber/parent` now
# resolves through the *weak* `parent` handle: it returns the parent iff its
# fiber state is still alive (rebuilt fresh, same identity), else nil — its
# documented contract. No strong child→parent edge is introduced, so a parent
# that captured its child does not form an unreclaimable RC cycle.

(def checked? (vm/config :checked-intrinsics))

# ── Leak bound: a completed-and-dropped fiber is reclaimed ──────────────
# Each iteration builds a fiber, resumes it to :dead, drops it. A correct
# runtime frees the fiber's region (and cascades its closure/template) at
# scope exit, so the live-object count over 2000 iters stays bounded and does
# NOT scale with the iteration count. Baseline leaked ~3/iter (delta ~6000).
(defn resume-churn [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn [] 7) 2)]
      (fiber/resume f))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d2000 (resume-churn 2000)]
  (assert (or checked? (%lt d2000 100))
          (string "fiber-resume object leak: 2000 iters grew live count by "
                  d2000 " (must stay bounded — Rule 8)")))

# Same property at the region granularity: each completed fiber's region must
# be reclaimed, so region-count growth is bounded too.
(defn resume-region-churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn [] 7) 2)]
      (fiber/resume f))
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d2000 (resume-region-churn 2000)]
  (assert (or checked? (%lt d2000 100))
          (string "fiber-resume region leak: 2000 iters grew region count by "
                  d2000 " (must stay bounded — Rule 8)")))

# ── Correctness: the fix must not change resume semantics ───────────────
# Releasing the carrier must not disturb the value the resume yields, nor the
# fiber's terminal state.
(assert (= (fiber/resume (fiber/new (fn [] (%add 40 2)) 2)) 42)
        "fiber/resume must still return the child's result")

(let [f (fiber/new (fn [] 7) 2)]
  (fiber/resume f)
  (assert (= (fiber/status f) :dead)
          "a fiber resumed to completion is :dead and readable afterward")
  (assert (= (fiber/value f) 7)
          "fiber/value reads the terminal result of a completed fiber"))

# ── Parent resolution after a completed resume must not UAF ─────────────
# `f` ran to completion INSIDE `outer`, so `f`'s parent is `outer`. Reading
# `(fiber/parent f)` after the resume must not touch a freed region. While
# `outer` is still alive (used after the read, below), `fiber/parent` returns
# it — same handle identity, rebuilt fresh. The whole tier runs clean under
# `--trace=guardfree` (the UAF oracle the leaked carrier used to hide).
(let [f (fiber/new (fn [] 42) 0)]
  (let [outer (fiber/new (fn []
                           (fiber/resume f)
                           99) 0)]
    (fiber/resume outer)
    (let [p (fiber/parent f)]
      (assert (fiber? p)
              "fiber/parent returns the parent while it is still alive")
      (assert (identical? p outer)
              "fiber/parent preserves the parent's identity (same handle)")  # Keep `outer` live across the parent read above so its region is not
      # reclaimed at the resume (its prior last use) before we read it.
      (fiber/status outer))))

# Identity stability regardless of liveness: two reads agree (both the live
# fiber, or both nil once the parent's region is reclaimed). Mirrors
# fibers.lisp's `test_fiber_parent_identity`.
(let [f (fiber/new (fn [] 42) 0)]
  (let [outer (fiber/new (fn []
                           (fiber/resume f)
                           99) 0)]
    (fiber/resume outer)
    (assert (identical? (fiber/parent f) (fiber/parent f))
            "fiber/parent identity is stable across repeated reads")))

# Churn: a completed nested parent/child pair, both dropped, must be fully
# reclaimed. Resolving the parent weakly (no strong child→parent edge) means
# the closure-captures-child / child-points-at-parent pair is NOT an
# unreclaimable RC cycle, so the live count stays bounded.
(defn parent-churn [n]
  (def before (arena/count))
  (def @i 0)
  (while (%lt i n)
    (let [f (fiber/new (fn [] 42) 0)]
      (let [outer (fiber/new (fn []
                               (fiber/resume f)
                               99) 0)]
        (fiber/resume outer)
        (fiber/parent f)))
    (assign i (%add i 1)))
  (%sub (arena/count) before))

(let [d2000 (parent-churn 2000)]
  (assert (or checked? (%lt d2000 100))
          (string "nested parent/child fiber leak: 2000 iters grew live count by "
                  d2000 " (no carrier leak, no parent RC cycle)")))
