(elle/epoch 12)
## parameter/creation-capture — a fiber captures its parameter environment at
## CREATION time, not at first resume. (RED counterfactual — known defect.)
##
## Spec (docs: dynamic parameters): when a fiber is created inside a
## `parameterize`, it snapshots the parameter bindings in effect AT CREATION.
## The bindings must survive even if the creating frame's `parameterize` has
## unwound before the fiber is first resumed — this is what makes `ev/spawn`
## work, where the spawner returns (and its `parameterize` pops) long before the
## scheduler resumes the child.
##
## Current behaviour (the defect this pins): parameter inheritance happens at
## first-resume from the resuming fiber, not at creation — so a child resumed
## AFTER its creator's `parameterize` has unwound sees the parameter's :default
## instead of the captured value. RED until the snapshot is taken at creation.
##
## Deliberately a SINGLE scenario: it records a plain assertion failure on each
## tier (vm + jit) without accumulating region state, so it pins the SEMANTICS
## cleanly. Accumulating several failing parameterize+fiber assertions in one
## file additionally trips a separate cumulative region use-after-free (the
## arena tag/object-mismatch abort — see DEVLOG and the full parameters.lisp
## run); that is a distinct defect and is NOT what this file is for.

(def p1 (parameter :default))
(let [f (parameterize ((p1 :inside))
          (fiber/new (fn () (p1)) 1))]
  (assert (= (p1) :default) "p1 reads :default outside parameterize")
  (fiber/resume f nil)
  (assert (= (fiber/value f) :inside)
          "fiber sees the creation-time snapshot (:inside), not :default"))

(println "parameter-creation-capture: ok")
