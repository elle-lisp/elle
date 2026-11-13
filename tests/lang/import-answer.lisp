(elle/epoch 16)
# audited: 2026-11-13
# An import's answer is [ok? value], from the loader *loader* holds where the import runs, under ev/run and lib/process alike.
# docs/modules.md
# docs/scheduler.md

# A fiber whose mask names :import answers the request with [false e], and the
# import raises e. The counter-factual answered the pair itself as the import's
# value.
(let [f (fiber/new (fn [] (protect (import "x"))) |:import|)]
  (fiber/resume f)
  (let [[ok? err] (fiber/resume f [false {:error :refused-here :message "no"}])]
    (assert (not ok?) "an answer of [false e] fails the import")
    (assert (= :refused-here (get err :error)) "and raises e")))

# The loader is the one *loader* holds where the import runs. The
# counter-factual kept the loader each fiber was spawned under, so a
# parameterize around the import itself did not reach it.
(parameterize ((*loader* (fn [_] :here)))
  (assert (= :here (import "std/anything"))
          "a parameterize around an import picks its loader"))

# ── A process's scheduler answers its sub-fibers' requests ─────────────

# lib/process runs a process's sub-fibers on a scheduler of its own, which
# answers a request as ev/run's does. The counter-factual kept the loader each
# sub-fiber was spawned under there too.
(def process ((import "std/process")))
(def answers @[])
(process:start (fn []
                 (push answers
                       (parameterize ((*loader* (fn [_] :spawned)))
                         (ev/join (ev/spawn (fn [] (import "x"))))))
                 (push answers
                       (ev/join-protected
                         (ev/spawn (fn []
                                     (parameterize ((*loader* (fn [_] :inner)))
                                       (import "x"))))))
                 (push answers
                       (ev/join-protected
                         (ev/spawn (fn []
                                     (parameterize ((*loader*
                                                      (fn [_]
                                                        (error
                                                          {:error :loader-said-no
                                                           :message "no"}))))
                                       (import "x"))))))))
(assert (= :spawned (get answers 0))
        "a sub-fiber starts with the loader its spawner held")
(assert (= [true :inner] (get answers 1))
        "a sub-fiber's parameterize around its import picks the loader")
(assert (= :loader-said-no (get (get (get answers 2) 1) :error))
        "a loader's error fails the sub-fiber's import")
