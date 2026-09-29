(elle/epoch 13)
# audited: 2026-09-29
# A vm/config answer in a spawned worker is born in its call's own region, never
# in the ambient one.
# docs/impl/region/rules.md
#
# `vm/config` and `vm/config-set` are SIG_QUERY primitives: the VM builds the
# answer, a set or a struct, while it answers the query. That answer is born in
# the call's own region like any native result (Rule 3). The ambient region is
# the placeholder for an opaque native call, and it holds no value the compiler
# knows about.
#
# The counter-factual: an answer built in the ambient region is harmless on the
# main thread, where the ambient holds only strays. A spawned worker runs its
# body with the region that holds the live closure and its captures as the
# ambient, so the answer's release drives that region to zero. The captures are
# freed mid-run, and the worker's cleanup frees them again.

# Heavy worker (sys/spawn, runs init_stdlib): churn the ambient region with many
# SIG_QUERY answers, then read a captured heap value. The capture must come back
# intact, and the worker must not crash on cleanup.
(let [cap [101 202 303 404 505]
      tag "captured-after-config"]
  (let [h (sys/spawn (fn []
                       (vm/config-set :trace ||)
                       (vm/config :trace)
                       (vm/config)
                       (vm/config :jit)
                       (vm/config-set :max-depth 10000000)
                       (vm/config :mlir)
                       (vm/config :trace)
                       (vm/config :stats)
                       [cap tag]))]
    (let [got (sys/join h)]
      (assert (= (get got 0) [101 202 303 404 505])
              "captured array survives ambient SIG_QUERY churn in worker")
      (assert (= (get got 1) "captured-after-config")
              "captured string survives ambient SIG_QUERY churn in worker"))))

# Light worker (sys/spawn-vm): the same property, with primitives only.
(let [cap @[1 2 3]]
  (let [h (sys/spawn-vm (fn []
                          (vm/config :trace)
                          (vm/config :jit)
                          (vm/config :trace)
                          cap))]
    (assert (= (sys/join h) @[1 2 3])
            "captured mutable array survives ambient SIG_QUERY churn (light worker)")))
