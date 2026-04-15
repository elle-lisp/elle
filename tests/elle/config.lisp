# vm/config tests
#
# Tests the unified runtime configuration system: vm/config read/write,
# trace keyword sets, JIT/WASM policy keywords, custom JIT policy.

# ── vm/config basics — reading the config struct ──────────────────────

# vm/config returns a struct
(let [[cfg (vm/config)]]
  (assert (struct? cfg) "vm/config returns a struct"))

# Reading specific fields
(assert (keyword? (vm/config :jit)) "vm/config :jit returns a keyword")
(assert (keyword? (vm/config :wasm)) "vm/config :wasm returns a keyword")

# Trace is a set (possibly empty)
(let [[t (vm/config :trace)]]
  (assert (set? t) "vm/config :trace returns a set"))

# ── JIT policy keywords ──────────────────────────────────────────────

# Save initial JIT policy (depends on CLI flags)
(def initial-jit (vm/config :jit))

# Setting JIT policy to :off
(vm/config-set :jit :off)
(assert (= (vm/config :jit) :off) "JIT policy set to :off")

# Setting JIT policy to :eager
(vm/config-set :jit :eager)
(assert (= (vm/config :jit) :eager) "JIT policy set to :eager")

# Setting JIT policy back to :adaptive
(vm/config-set :jit :adaptive)
(assert (= (vm/config :jit) :adaptive) "JIT policy restored to :adaptive")

# ── WASM policy keywords ─────────────────────────────────────────────

# Default WASM policy is :off
(assert (= (vm/config :wasm) :off) "default WASM policy is :off")

# Setting WASM policy
(vm/config-set :wasm :full)
(assert (= (vm/config :wasm) :full) "WASM policy set to :full")

(vm/config-set :wasm :lazy)
(assert (= (vm/config :wasm) :lazy) "WASM policy set to :lazy")

# Restore
(vm/config-set :wasm :off)
(assert (= (vm/config :wasm) :off) "WASM policy restored to :off")

# ── Trace keyword sets ────────────────────────────────────────────────

# Initially empty (no --trace flag)
(assert (empty? (vm/config :trace)) "trace set initially empty")

# Setting trace keywords
(vm/config-set :trace |:call|)
(assert (contains? (vm/config :trace) :call) "trace set contains :call after set")

# Multiple trace keywords
(vm/config-set :trace |:call :signal :fiber|)
(let [[t (vm/config :trace)]]
  (assert (contains? t :call) "trace set contains :call")
  (assert (contains? t :signal) "trace set contains :signal")
  (assert (contains? t :fiber) "trace set contains :fiber"))

# Clearing trace
(vm/config-set :trace ||)
(assert (empty? (vm/config :trace)) "trace set cleared")

# ── Future feature flags — accepted without error ─────────────────────

# These keywords should be accepted in trace sets without error,
# even though the subsystems don't exist yet.
(vm/config-set :trace |:spirv :mlir :gpu|)
(let [[t (vm/config :trace)]]
  (assert (contains? t :spirv) "future flag :spirv accepted")
  (assert (contains? t :mlir) "future flag :mlir accepted")
  (assert (contains? t :gpu) "future flag :gpu accepted"))

# Clean up
(vm/config-set :trace ||)

# ── Custom JIT policy via closure ─────────────────────────────────────

# Set a custom JIT policy closure
(vm/config-set :jit
  (fn [info]
    (if (and (get info :silent) (> (get info :calls) 5))
      :jit
      :skip)))

# Custom policy should report as :custom
(assert (= (vm/config :jit) :custom) "custom JIT policy reports as :custom")

# Restore
(vm/config-set :jit :adaptive)

# ── Config struct fields ──────────────────────────────────────────────

# The full config struct should have expected keys
(let [[cfg (vm/config)]]
  (assert (has-key? cfg :jit) "config has :jit key")
  (assert (has-key? cfg :wasm) "config has :wasm key")
  (assert (has-key? cfg :trace) "config has :trace key")
  (assert (has-key? cfg :stats) "config has :stats key"))

# ── Boolean config fields ────────────────────────────────────────────

(let [[cfg (vm/config)]]
  # stats defaults to false
  (assert (= (get cfg :stats) false) "stats defaults to false"))
