(elle/epoch 12)
## region/closure-template — a closure TEMPLATE is an ordinary region allocation
## reclaimed by region RC (a heap literal is an ordinary, reclaimable
## allocation; closure templates are no exception). Stage 3 Phase 2b.
##
## Two properties this pins:
##
##  (a) CORRECTNESS — the fiber/region-template edge. A `MakeClosure` now
##      materializes the template into the SAME region as the closure instance
##      (co-region). A fiber holds its closure BY VALUE, so the fiber's region
##      scan must keep that template region alive itself — especially for an
##      EMPTY-env closure, where there is no captured-env backing to ride on.
##      Counterfactual (RED before the Fiber-arm template scan in
##      `find_object_cross_refs`): the fiber's template region is freed while the
##      parked fiber still needs it, and the first resume reads a torn template /
##      empty env — a use-after-free, NOT a leak. A nested fiber (one fiber
##      resuming another, both empty-env) is the minimal trigger.
##
##  (b) NO LEAK — re-materializing a template per execution must be reclaimed.
##      Creating and dropping many closures/fibers leaves the live object count
##      flat; a template's region left unfreed would grow it ~1+/iter (Rule 8).

# ── (a) correctness: nested empty-env fibers resume without UAF ────────
# `f` is an empty-env closure with a Region template in its own region; `w`
# captures `f` and resumes it. `f`'s yield (mask 0) propagates to `w` (mask 2),
# which catches it, so `(fiber/resume w)` yields 42. Before the fix this reads a
# freed template region ("Upvalue index 0 out of bounds (env size: 0)").
(defn nested-resume ()
  (let [f (fiber/new (fn [] (yield 42)) 0)]
    (let [w (fiber/new (fn [] (fiber/resume f)) 2)]
      (fiber/resume w))))

(assert (= (nested-resume) 42)
        "nested empty-env fiber resume read a freed closure-template region (UAF)")

# A generator closure that captures a value, yields it across a suspend, and
# resumes — exercises the template region surviving a suspend/resume cycle.
(defn gen-from (start)
  (fiber/new (fn []
               (yield start)
               (yield (%add start 1))
               (%add start 2)) 2))

(let [g (gen-from 10)]
  (assert (= (fiber/resume g) 10) "generator: first yield")
  (assert (= (fiber/resume g) 11) "generator: second yield")
  (assert (= (fiber/resume g) 12) "generator: final return"))

# NOTE on leak coverage: the template *reclamation* witness (a non-fiber closure
# churn leaving a flat object-count delta) lives in tests/elle/oracle.lisp as the
# `closure-template` probe — placed there so it runs alongside the other
# reclamation classes. It is deliberately NOT repeated here with FIBERS: a fiber
# resumed-to-completion is itself currently not reclaimed (a pre-existing fiber
# leak, orthogonal to closure templates), and pinning the closure's template
# region to that leaked fiber — required for the correctness above — would make a
# fiber-churn count grow for a reason unrelated to template reclamation. This
# file therefore pins only the fiber/region-template *correctness* (no UAF).

(println "region-closure-template: ok")
