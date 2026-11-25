(elle/epoch 18)
# audited: 2026-11-23
## elle test — the build's tiers: which backends a form runs on, and which JIT
## policies a whole-file script runs under.
## docs/test-runner.md
##
## A fragment of one module (see store.lisp).

# compile/run-on answers :tier-rejected/:feature-disabled for a tier whose
# feature wasn't compiled in. Such a tier is dropped from the run entirely (a
# feature the binary lacks is not a coverage gap of THIS build). A tier that is
# present but can't run a particular form answers :ineligible — that is a
# per-form skip (see classify), not an absent tier.
(defn feature-disabled? [r]
  (and (not (get r 0)) (= (get (get r 1) :error) :tier-rejected)
       (= (get (get r 1) :reason) :feature-disabled)))

# A tier is available when a trivial closure runs on it.
(defn tier-available? [tk]
  (not (feature-disabled? (exec-thunk tk (fn [] 0)))))

# Candidate tiers as [tier-keyword tier-label]; :bytecode is recorded as :vm.
# The label is a keyword (result.tier is keyword-typed; sqlite stores its name).
(def candidate-tiers
  [[:bytecode :vm] [:jit :jit] [:wasm :wasm] [:mlir-cpu :mlir-cpu]])

(def active-tiers (filter (fn [p] (tier-available? (get p 0))) candidate-tiers))

# A whole-file (legacy multi-form) thunk is a yielding imperative script: it runs
# under the worker's full scheduler, NOT forced onto a backend via compile/run-on
# (that only fits a single non-yielding form). So instead of a tier we vary the
# JIT POLICY it runs under: :off (pure bytecode, recorded "vm") and :eager (JIT
# every function, recorded "jit"). Each worker sets it via (vm/config-set :jit
# …). :eager is included only when this build carries the JIT. Each entry is
# [policy label].
# No value-divergence is judged across policies (process-whole passes diverge?
# false): a script's pids/timestamps differ run-to-run by design.
(def whole-file-policies
  (concat [[:off :vm]] (if (tier-available? :jit) [[:eager :jit]] [])))

# Comma-joined tier labels for the run.tiers column. Labels are keywords, so
# stringify each (`(string :vm)` → "vm").
(defn tiers-str [tiers]
  (if (empty? tiers)
    ""
    (if (empty? (rest tiers))
      (string (get (first tiers) 1))
      (string (string (get (first tiers) 1)) "," (tiers-str (rest tiers))))))
