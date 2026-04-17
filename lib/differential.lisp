## lib/differential.lisp — cross-tier agreement harness
##
## Force-runs a closure on every compilation tier (bytecode, JIT,
## MLIR-CPU) and asserts they produce the same result. A disagreement
## is always a bug in the lowering, the eligibility predicate, the
## tier dispatch, or the underlying engine.
##
## Built on (compile/run-on tier f & args). See docs/impl/differential.md.
##
## Usage:
##   (def diff ((import "std/differential")))
##
##   (diff:assert-agree (fn [a b] (* a (+ b 1))) 3 7)
##
##   (def report (diff:call (fn [x] (- 0 x)) -42))
##   ## → {:agreed true :tiers |:bytecode :jit| :result 42}

(fn []

## ── Tiers ───────────────────────────────────────────────────────────

## Every tier we attempt. Order matters: :bytecode is the reference.
(def all-tiers [:bytecode :jit :mlir-cpu])

## ── Try-on-tier ─────────────────────────────────────────────────────

## Run f on a single tier, returning [:ok result] on success or
## [:skipped reason] when the tier rejects the closure or feature.
## Errors that aren't tier rejections propagate as [:error err].
(defn try-on [tier f & args]
  (let [[[ok? result] (protect (apply compile/run-on tier f args))]]
    (if ok?
      [:ok result]
      ## protect returned the error value; classify it.
      (let [[err-kind (and (struct? result) (get result :error))]]
        (if (= err-kind :tier-rejected)
          [:skipped result]
          [:error result])))))

## ── eligible-tiers ──────────────────────────────────────────────────

## Probe each tier with the given args; return a set of tiers that
## successfully executed f. Useful for diagnostics.
(defn eligible-tiers [f & args]
  (var ts (set))
  (each tier in all-tiers
    (let [[[status _] (apply try-on tier f args)]]
      (when (= status :ok)
        (assign ts (add ts tier)))))
  ts)

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
(defn call [f & args]
  (var results @{})
  (var skipped @{})
  (var errors  @{})
  (each tier in all-tiers
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
               [agreed?   (all? (fn [t] (= (results t) ref-value))
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

## ── Export ──────────────────────────────────────────────────────────

{:tiers          all-tiers
 :try-on         try-on
 :eligible-tiers eligible-tiers
 :call           call
 :assert-agree   assert-agree
 :format-disagreement format-disagreement})
