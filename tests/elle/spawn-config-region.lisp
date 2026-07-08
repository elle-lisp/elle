(elle/epoch 12)
# Regression: a SIG_QUERY result is born in its call's own region, never the
# ambient TLS region.
#
# `vm/config` / `vm/config-set` are SIG_QUERY primitives: the *VM* builds their
# result (a set / struct) while answering the query. That answer must be born in
# the call's solver-assigned region (Rule 3 — values are born in their own
# region), like any native-call result. The defect resolved the query in the
# caller's `SignalAction::Query` arm, where the active region is the ambient TLS
# region — which docs/impl/region/rules.md reserves for the opaque-native-fn placeholder,
# "no business holding a compiler-known [VM-built] value".
#
# On the main thread that was benign: the ambient holds only strays, so the
# result's `DecrefValueRegion` just churns a disposable region. In a spawned
# worker it was fatal: the worker runs the body with its reconstruction region
# (holding the live closure + captures) as the ambient, so the result's decref
# drove that region to RC 0, freeing the captures mid-run and double-freeing at
# the worker's cleanup decref (the `make smoke-elle` UAF: a `tag/object mismatch
# — use-after-free` torn read, or in debug the `DecrefRegion(N) but region was
# never alloc_in_region'd` phantom/double-free panic).
#
# The fix is in the VM (`dispatch_native_call`): resolve SIG_QUERY inside the
# call's region. See docs/impl/region/model.md § "Constants lower as ordinary
# allocations" (the ambient TLS is the opaque-call placeholder) and Rule 3.

# Heavy worker (sys/spawn, runs init_stdlib): drain the ambient region with many
# SIG_QUERY allocations, then read a captured heap value. The capture must come
# back intact (and the worker must not crash on cleanup).
(let [cap [101 202 303 404 505]
      tag "captured-after-config"]
  (let [h (sys/spawn (fn []
                       (vm/config-set :jit :off)
                       (vm/config :trace)
                       (vm/config-set :jit :eager)
                       (vm/config :jit)
                       (vm/config-set :jit :adaptive)
                       (vm/config :wasm)
                       (vm/config :trace)
                       (vm/config :stats)
                       [cap tag]))]
    (let [got (sys/join h)]
      (assert (= (get got 0) [101 202 303 404 505])
              "captured array survives ambient SIG_QUERY churn in worker")
      (assert (= (get got 1) "captured-after-config")
              "captured string survives ambient SIG_QUERY churn in worker"))))

# Light worker (sys/spawn-vm) variant — same property, primitives-only env.
(let [cap @[1 2 3]]
  (let [h (sys/spawn-vm (fn []
                          (vm/config :trace)
                          (vm/config :jit)
                          (vm/config :trace)
                          cap))]
    (assert (= (sys/join h) @[1 2 3])
            "captured mutable array survives ambient SIG_QUERY churn (light worker)")))
