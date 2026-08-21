(elle/epoch 12)
# Trace output tests
#
# Tests vm/config-set :trace behavior from Elle code.
# CLI --trace flag testing is done via the Makefile smoke targets.

# ── Trace baseline ────────────────────────────────────────────────────
# The harness may start this process with trace flags of its own
# (ELLE_TEST_FLAGS=--trace=...), so the initial set is a baseline to
# restore at the end, never asserted empty. What the test owns is the
# keywords it sets itself: none of them may be pre-set, or the
# contains? assertions below would pass vacuously.

(def baseline (vm/config :trace))
(assert (not (contains? baseline :call)) "test keyword :call not pre-set")
(assert (not (contains? baseline :signal)) "test keyword :signal not pre-set")
(assert (not (contains? baseline :fiber)) "test keyword :fiber not pre-set")

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
(defn traced-fn [x]
  (+ x 1))
(assert (= (traced-fn 5) 6) "traced function works correctly")
(vm/config-set :trace ||)

# ── All known keywords ────────────────────────────────────────────────

(vm/config-set :trace |:call :signal :compile :fiber :hir :lir :emit :jit :io
                       :gc :import :macro :wasm :capture :arena :escape
                       :bytecode|)
(let [t (vm/config :trace)]
  (assert (>= (length t) 17) "all 17 known keywords accepted"))

# ── Restore the baseline ──────────────────────────────────────────────
# vm/config-set :trace replaces the whole set, so every set above
# dropped the harness's own flags; hand them back.

(vm/config-set :trace baseline)
