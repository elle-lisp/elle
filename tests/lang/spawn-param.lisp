(elle/epoch 12)
## spawn-param — an `ev/spawn`ed fiber must see the spawner's dynamic
## parameter bindings. (RED counterfactual — known defect.)
##
## Spec (docs: dynamic parameters): a fiber snapshots the parameter bindings
## in effect at CREATION, so a child the scheduler resumes later — after the
## spawner's `parameterize` frame is no longer the resumer's — still reads
## the spawned-time value. This is the `redis:with` + `ev/spawn` shape: the
## connection parameter is bound on the spawner, and the scheduler (whose own
## param frames are empty) performs the resume.
##
## Current behaviour (the defect this pins): parameter inheritance happens at
## first-resume from the RESUMING fiber. The scheduler is the resumer here,
## so the child sees the parameter's default even while the spawner's binding
## is still live at the `ev/join`. RED until the snapshot is taken at
## creation — the same defect parameter-creation-capture.lisp pins in its
## direct `fiber/new` form; this file pins the ev/spawn face of it (the one
## production hits — redis.lisp's spawn-canary section fails on it whenever a
## live Redis lets that section run).
##
## Deliberately a SINGLE scenario, for the same reason as
## parameter-creation-capture.lisp: one plain assertion failure per tier pins
## the semantics without accumulating the parameterize+fiber region state
## that trips a separate cumulative use-after-free.

(def p (parameter :default))
(parameterize ((p :bound))
  (let [result-box (box nil)
        f (ev/spawn (fn () (rebox result-box (p))))]
    (ev/join f)
    (assert (= (unbox result-box) :bound)
            "an ev/spawned fiber sees the spawner's parameter binding")))

(println "spawn-param: ok")
