(elle/epoch 13)
## audited: 2026-09-29
## Fixture: a whole-file script observes the JIT policy the runner set for it.
## docs/test-runner.md
##
## The runner runs a multi-form file with the JIT off, where `(vm/config :jit)`
## reads nil, and with the JIT eager, where it reads 0. A runner that only
## labelled its rows and set no policy leaves the build's threshold, 10, and
## the file fails under both.
(def threshold (vm/config :jit))
(assert (or (nil? threshold) (= threshold 0))
        (string "the runner set no JIT policy: (vm/config :jit) is " threshold))
