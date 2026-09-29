(elle/epoch 13)
# audited: 2026-09-29
# A fiber keeps its closure's template region alive, so a parked empty-env closure resumes intact.
# docs/impl/region/template.md
#
# A `MakeClosure` materializes the template into the SAME region as the
# closure instance. A fiber holds its closure BY VALUE, so the fiber's region
# scan (the Fiber arm of `find_object_cross_refs`,
# src/value/fiberheap/regionpool/introspect.rs) keeps that template region
# alive itself. An EMPTY-env closure has no captured-env backing to ride on,
# which makes it the case the edge exists for.
#
# The counter-factual: a Fiber arm with no template edge frees the fiber's
# template region while the parked fiber still needs it, and the first resume
# reads a torn template and an empty env. A nested fiber, one fiber resuming
# another with both closures empty-env, is the smallest shape that reaches it.

# `f` is an empty-env closure with a Region template in its own region; `w`
# captures `f` and resumes it. `f`'s yield (mask 0) propagates to `w` (mask 2),
# which catches it, so `(fiber/resume w)` yields 42. A freed template region
# fails with "Upvalue index 0 out of bounds (env size: 0)".
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

# The template's reclamation is the oracle's `closure-template` row
# (tests/impl/probe/direct.lisp), which churns closures without fibers. A
# fiber pins its closure's template region to itself, so a count over fiber
# churn measures the fiber's reclamation rather than the template's.

(println "region-closure-template: ok")
