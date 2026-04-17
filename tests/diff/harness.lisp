## tests/diff/harness.lisp — cross-tier agreement test harness
##
## Force-runs a closure on every compilation tier (bytecode, JIT,
## WASM, MLIR-CPU, GPU) and asserts they produce the same result.
## A disagreement is always a bug in the lowering, the eligibility
## predicate, the tier dispatch, or the underlying engine.
##
## Built on (compile/run-on tier f & args). See docs/impl/differential.md.
##
## Usage:
##   (def diff ((import "tests/diff/harness")))
##
##   (diff:assert-agree (fn [a b] (* a (+ b 1))) 3 7)
##
##   (def report (diff:call (fn [x] (- 0 x)) -42))
##   ## → {:agreed true :tiers |:bytecode :jit| :result 42}
##
##   ## Property-based: 100 random inputs
##   (diff:prop (fn [a b] (+ a b))
##              (fn [] [(rng:int -1000 1000) (rng:int -1000 1000)]))
##
##   ## Float tolerance: set epsilon for approximate float comparison
##   (diff:with-tolerance 1e-10)
##   (diff:assert-agree (fn [x] (* x 0.1)) 3.0)
##   (diff:with-tolerance nil)  ## back to exact

(fn []

## ── Tiers ───────────────────────────────────────────────────────────

## Every tier we attempt. Order matters: :bytecode is the reference.
## The :gpu tier is opt-in: callers pass a gpu-lib via with-gpu to
## enable it, since loading the vulkan plugin requires runtime setup
## that the library can't do at compile time.
(def all-tiers [:bytecode :jit :wasm :mlir-cpu])

## Mutable: callers can extend via with-gpu.
(var gpu-lib nil)

## ── with-gpu ─────────────────────────────────────────────────────────

## Register the gpu library for GPU-tier testing. Callers that have
## already imported plugin/vulkan and std/gpu pass the gpu lib here:
##   (diff:with-gpu ((import "std/gpu")))
## After this, call/assert-agree will also test the :gpu tier.
(defn with-gpu [lib]
  (assign gpu-lib lib))

## ── Try-on-tier ─────────────────────────────────────────────────────

## Run f on a single tier, returning [:ok result] on success or
## [:skipped reason] when the tier rejects the closure or feature.
## Errors that aren't tier rejections propagate as [:error err].
(defn try-on [tier f & args]
  (if (= tier :gpu)
    ## GPU tier routes through gpu:map.
    (if (nil? gpu-lib)
      [:skipped {:error :tier-rejected
                 :message "GPU not registered (call diff:with-gpu first)"
                 :tier :gpu
                 :reason :ineligible}]
      (let [[[ok? result] (protect
                            (let [[inputs (map (fn [a] [a]) args)]]
                              (first (apply (get gpu-lib :map) f inputs))))]]
        (if ok?
          [:ok result]
          (let [[err-kind (and (struct? result) (get result :error))]]
            (if (= err-kind :tier-rejected)
              [:skipped result]
              [:error result])))))
    ## Non-GPU tiers: use compile/run-on.
    (let [[[ok? result] (protect (apply compile/run-on tier f args))]]
      (if ok?
        [:ok result]
        (let [[err-kind (and (struct? result) (get result :error))]]
          (if (= err-kind :tier-rejected)
            [:skipped result]
            [:error result]))))))

## ── eligible-tiers ──────────────────────────────────────────────────

## The set of tiers to attempt: base tiers + :gpu if registered.
(defn active-tiers []
  (if (nil? gpu-lib) all-tiers (concat all-tiers [:gpu])))

## Probe each tier with the given args; return a set of tiers that
## successfully executed f. Useful for diagnostics.
(defn eligible-tiers [f & args]
  (var ts (set))
  (each tier in (active-tiers)
    (let [[[status _] (apply try-on tier f args)]]
      (when (= status :ok)
        (assign ts (add ts tier)))))
  ts)

## ── Float tolerance ──────────────────────────────────────────────────

## Module-level epsilon for float comparison. nil = exact equality.
## Set with (diff:with-tolerance 1e-10), clear with (diff:with-tolerance nil).
(var tolerance nil)

(defn with-tolerance [eps]
  (assign tolerance eps))

## Check if two floats are within epsilon of each other.
(defn float-close? [a b eps]
  (< (abs (- a b)) eps))

## Compare two values for agreement. When epsilon is nil, use exact
## equality. When epsilon is set, use approximate comparison for
## float pairs; all other types still use exact equality.
(defn values-agree? [a b epsilon]
  (if (nil? epsilon)
    (= a b)
    (if (and (float? a) (float? b))
      (float-close? a b epsilon)
      (= a b))))

## ── call ────────────────────────────────────────────────────────────

## Run f on every tier; compare results.
##
## Returns a struct:
##   {:agreed bool
##    :tiers <set of tiers that ran>
##    :results {tier value ...}      ## per-tier values when disagreed
##    :result <value>                 ## reference value when agreed
##    :skipped {tier rejection ...}   ## tiers that refused this closure
##    :errors  {tier error ...}       ## tiers that failed unexpectedly
##    :ref-tier <tier>                ## tier used as the reference}
##
## Float comparison uses the module-level tolerance (set via with-tolerance).
(defn call [f & args]
  (var results @{})
  (var skipped @{})
  (var errors  @{})
  (each tier in (active-tiers)
    (let [[[status payload] (apply try-on tier f args)]]
      (cond
        ((= status :ok)      (put results tier payload))
        ((= status :skipped) (put skipped tier payload))
        ((= status :error)   (put errors  tier payload)))))
  (let* [[ran-tiers   (keys results)]
         [num-results (length ran-tiers)]]
    (cond
      ## No tier ran — caller almost certainly made a mistake.
      ((= num-results 0)
        {:agreed false
         :tiers (set)
         :results @{}
         :skipped skipped
         :errors  errors
         :ref-tier nil})
      ## Single tier — trivially agreed.
      ((= num-results 1)
        {:agreed true
         :tiers (apply set ran-tiers)
         :result (results (first ran-tiers))
         :ref-tier (first ran-tiers)
         :skipped skipped
         :errors  errors})
      ## Multiple tiers — compare all against the reference.
      (true
        (let* [[ref-tier  (if (contains? results :bytecode)
                            :bytecode
                            (first ran-tiers))]
               [ref-value (results ref-tier)]
               [agreed?   (all? (fn [t] (values-agree? (results t) ref-value tolerance))
                                ran-tiers)]]
          (if agreed?
            {:agreed true
             :tiers (apply set ran-tiers)
             :result ref-value
             :ref-tier ref-tier
             :skipped skipped
             :errors  errors}
            {:agreed false
             :tiers (apply set ran-tiers)
             :results results
             :ref-tier ref-tier
             :skipped skipped
             :errors  errors}))))))

## ── format-disagreement ─────────────────────────────────────────────

## Pretty-print a disagreement report for assertion failures.
(defn format-disagreement [report f args]
  (var lines @[])
  (push lines "tier disagreement under (compile/run-on ...)")
  (push lines (string "  closure:  " f))
  (push lines (string "  args:     " args))
  (push lines (string "  ran on:   " (get report :tiers)))
  (push lines (string "  reference: " (get report :ref-tier)))
  (push lines "  results:")
  (each tier in (sort (keys (get report :results)))
    (push lines (string "    " tier " → " ((get report :results) tier))))
  (when (not (empty? (get report :skipped)))
    (push lines "  skipped:")
    (each tier in (sort (keys (get report :skipped)))
      (push lines (string "    " tier " ← "
                          (get ((get report :skipped) tier) :message)))))
  (when (not (empty? (get report :errors)))
    (push lines "  errors:")
    (each tier in (sort (keys (get report :errors)))
      (push lines (string "    " tier " ← " ((get report :errors) tier)))))
  (string/join lines "\n"))

## ── assert-agree ────────────────────────────────────────────────────

## Test helper: signal :diff-disagreement if any eligible tier returns
## a different value. Returns the agreed value on success.
##
## Float comparison uses the module-level tolerance (set via with-tolerance).
(defn assert-agree [f & args]
  (let [[report (apply call f args)]]
    (cond
      ((not (get report :agreed))
        (error {:error :diff-disagreement
                :message (format-disagreement report f args)
                :report report}))
      ((empty? (get report :tiers))
        (error {:error :diff-no-tiers
                :message "no tier accepted this closure"
                :skipped (get report :skipped)
                :errors  (get report :errors)}))
      (true
        (get report :result)))))

## ── prop ──────────────────────────────────────────────────────────────

## Property-based agreement: run assert-agree n times with random args
## generated by gen-fn. gen-fn must return a list of arguments.
##
##   (diff:prop (fn [a b] (+ a b))
##              (fn [] [(rng:int -1000 1000) (rng:int -1000 1000)])
##              :n 200)
(defn prop [f gen-fn &named n]
  (default n 100)
  (each _ in (range n)
    (let [[args (gen-fn)]]
      (apply assert-agree f args))))

## ── Export ──────────────────────────────────────────────────────────

{:tiers          all-tiers
 :active-tiers   active-tiers
 :with-gpu       with-gpu
 :try-on         try-on
 :eligible-tiers eligible-tiers
 :call           call
 :assert-agree   assert-agree
 :format-disagreement format-disagreement
 :prop           prop
 :with-tolerance with-tolerance
 :float-close?   float-close?
 :values-agree?  values-agree?})
