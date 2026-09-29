(elle/epoch 13)
# audited: 2026-09-29
# vm/config on this implementation: the tier thresholds, what the setter
# refuses, the trace set, and the keys of the struct.
# docs/config.md
#
# The rig runs this file under the build's defaults, and again under the eager
# profile, so the JIT threshold it first reads is the build's own or 0.

(assert (struct? (vm/config)) "vm/config returns a struct")

# Run `thunk` and answer the error kind it raised, or nil when it returned.
(defn raised [thunk]
  (let [[ok? err] (protect (thunk))]
    (if ok? nil (get err :error))))

# ── The JIT threshold ───────────────────────────────────────────────────

(def initial-jit (vm/config :jit))
(def jit-tier? (not (nil? initial-jit)))

(assert (or (nil? initial-jit) (= initial-jit 10) (= initial-jit 0))
        (string "(vm/config :jit) reads nil, the build's 10, or the rig's eager 0; got "
                initial-jit))

# The type is checked before the build: a keyword is the wrong type whether or
# not this build carries the tier. The counter-factual is the policy keyword
# API, where :off and :eager were the settings a program chose.
(assert (= (raised (fn [] (vm/config-set :jit :off))) :type-error)
        "a program cannot turn the JIT off")
(assert (= (raised (fn [] (vm/config-set :jit :eager))) :type-error)
        "a program cannot make the JIT eager")
(assert (= (raised (fn [] (vm/config-set :jit 2.5))) :type-error)
        "a threshold is an integer")

# A closure chose each function's tier under the custom policy, which is gone.
(assert (= (raised (fn [] (vm/config-set :jit (fn [info] :jit)))) :type-error)
        "a closure is not a threshold")

(if jit-tier?
  (begin
    (assert (= (raised (fn [] (vm/config-set :jit 0))) :argument-error)
            "a threshold below one compiles nothing it could name")
    (assert (= (raised (fn [] (vm/config-set :jit -2))) :argument-error)
            "a negative threshold is refused")
    (vm/config-set :jit 5)
    (assert (= (vm/config :jit) 5) "the threshold reads back as set")
    (vm/config-set :jit 1)
    (assert (= (vm/config :jit) 1)
            "a threshold of one compiles on the first call"))
  (assert (= (raised (fn [] (vm/config-set :jit 5))) :argument-error)
          "a build without the JIT refuses a threshold"))

# ── The MLIR threshold ──────────────────────────────────────────────────

(def mlir (vm/config :mlir))
(assert (or (nil? mlir) (and (integer? mlir) (>= mlir 0)))
        (string "(vm/config :mlir) reads nil or a threshold; got " mlir))
(assert (= (raised (fn [] (vm/config-set :mlir :eager))) :type-error)
        "a program cannot make the MLIR tier eager")
(when (nil? mlir)
  (assert (= (raised (fn [] (vm/config-set :mlir 5))) :argument-error)
          "a build without the MLIR tier refuses a threshold"))

# ── The WASM policy exists only in a wasm build ─────────────────────────

(if (has-key? (vm/config) :wasm)
  (assert (keyword? (vm/config :wasm)) "a wasm build reads its policy keyword")
  (assert (= (raised (fn [] (vm/config :wasm))) :argument-error)
          "a build without the WASM backend has no :wasm key"))

# ── Trace keyword sets ──────────────────────────────────────────────────

# A sidecar or a profile may start this process with trace keywords of its
# own, so the initial set is a baseline to restore at the end of this
# section, asserted only to lack the keywords the section sets itself.
(def saved-trace (vm/config :trace))
(assert (set? saved-trace) "vm/config :trace returns a set")
(assert (not (contains? saved-trace :call)) "test keyword :call not pre-set")
(assert (not (contains? saved-trace :signal)) "test keyword :signal not pre-set")
(assert (not (contains? saved-trace :fiber)) "test keyword :fiber not pre-set")

(vm/config-set :trace |:call|)
(assert (contains? (vm/config :trace) :call)
        "trace set contains :call after set")

(vm/config-set :trace |:call :signal :fiber|)
(let [t (vm/config :trace)]
  (assert (contains? t :call) "trace set contains :call")
  (assert (contains? t :signal) "trace set contains :signal")
  (assert (contains? t :fiber) "trace set contains :fiber"))

(vm/config-set :trace ||)
(assert (empty? (vm/config :trace)) "trace set cleared")

# These keywords are accepted in a trace set although their subsystems do not
# exist yet (docs/config.md).
(vm/config-set :trace |:spirv :mlir :gpu|)
(let [t (vm/config :trace)]
  (assert (contains? t :spirv) "future flag :spirv accepted")
  (assert (contains? t :mlir) "future flag :mlir accepted")
  (assert (contains? t :gpu) "future flag :gpu accepted"))

# `vm/config-set :trace` replaces the whole set, so every set above dropped
# the keywords the run started with.
(vm/config-set :trace saved-trace)

# ── The struct ──────────────────────────────────────────────────────────

(let [cfg (vm/config)]
  (assert (has-key? cfg :jit) "config has :jit key")
  (assert (has-key? cfg :mlir) "config has :mlir key")
  (assert (has-key? cfg :trace) "config has :trace key")
  (assert (has-key? cfg :stats) "config has :stats key")
  (assert (has-key? cfg :max-depth) "config has :max-depth key")
  (assert (= (get cfg :stats) false)
          "statistics are off unless --dump=stats asks"))

# An unknown field is refused, never read as nil.
(assert (= (raised (fn [] (vm/config :no-such-field))) :argument-error)
        "an unknown field is an argument error")
(assert (= (raised (fn [] (vm/config-set :no-such-field 1))) :argument-error)
        "setting an unknown field raises")
