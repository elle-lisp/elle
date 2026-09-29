(elle/epoch 13)
## audited: 2026-09-29
## Fixture: a file reads the JIT policy its build ships, because the runner
## sets none.
## docs/test-runner.md
##
## `(vm/config :jit)` reads the build's threshold, 10, or nil in a build that
## carries no JIT. A runner that ran the file with the JIT off reads nil and one
## that made it eager reads 0, so the second fails here.
(def threshold (vm/config :jit))
(assert (or (nil? threshold) (= threshold 10))
        (string "the runner forced a JIT policy: (vm/config :jit) is " threshold))
