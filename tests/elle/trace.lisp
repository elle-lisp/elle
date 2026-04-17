(elle/epoch 7)
# Trace output tests
#
# Tests vm/config-set :trace behavior from Elle code.
# CLI --trace flag testing is done via the Makefile smoke targets.

# ── Trace set starts empty ────────────────────────────────────────────

(assert (empty? (vm/config :trace)) "trace set initially empty")

# ── Setting and clearing trace keywords ───────────────────────────────

(vm/config-set :trace |:call|)
(assert (contains? (vm/config :trace) :call) "trace :call enabled")

(vm/config-set :trace |:call :signal :fiber|)
(let [t (vm/config :trace)]
  (assert (contains? t :call) "multi: :call")
  (assert (contains? t :signal) "multi: :signal")
  (assert (contains? t :fiber) "multi: :fiber")
  (assert (not (contains? t :jit)) "multi: :jit not set"))

(vm/config-set :trace ||)
(assert (empty? (vm/config :trace)) "trace cleared")

# ── Future keywords accepted without error ────────────────────────────

(vm/config-set :trace |:spirv :mlir :gpu|)
(let [t (vm/config :trace)]
  (assert (contains? t :spirv) ":spirv accepted")
  (assert (contains? t :mlir) ":mlir accepted")
  (assert (contains? t :gpu) ":gpu accepted"))
(vm/config-set :trace ||)

# ── Trace enable mid-program ─────────────────────────────────────────
# After enabling :call trace, function calls should produce trace output.
# We can't easily capture our own stderr in-process, but we can verify
# that the config state is correct and doesn't crash.

(vm/config-set :trace |:call|)
(defn traced-fn [x] (+ x 1))
(assert (= (traced-fn 5) 6) "traced function works correctly")
(vm/config-set :trace ||)

# ── All known keywords ────────────────────────────────────────────────

(vm/config-set :trace |:call :signal :compile :fiber :hir :lir :emit :jit
                        :io :gc :import :macro :wasm :capture :arena :escape
                        :bytecode|)
(let [t (vm/config :trace)]
  (assert (>= (length t) 17) "all 17 known keywords accepted"))
(vm/config-set :trace ||)
