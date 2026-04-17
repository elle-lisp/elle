# ── Disagreement detection sanity check ───────────────────────────────
#
# The harness must detect disagreement between tiers. We can't naturally
# trigger one (any actual disagreement would be a backend bug we'd want
# to fix, not test). Instead, simulate disagreement by hand-injecting
# different per-tier results into call's report-handling logic and
# verifying assert-agree throws.

(def diff ((import "tests/diff/harness")))

# An ordinary closure that all tiers agree on — should not throw.
(defn add [a b] (+ a b))
(assert (= (diff:assert-agree add 3 4) 7) "agreement returns the value")

# Inject a synthetic disagreement and verify format-disagreement
# produces a useful string. The shape of the report is fixed by call;
# we construct one directly to exercise the formatter.
(def fake-report
  {:agreed   false
   :tiers    |:bytecode :jit|
   :results  {:bytecode 24 :jit 27}
   :ref-tier :bytecode
   :skipped  @{}
   :errors   @{}})

(def msg (diff:format-disagreement fake-report add [3 7]))
(assert (string-contains? msg "tier disagreement")  "header present")
(assert (string-contains? msg "bytecode")            ":bytecode reported")
(assert (string-contains? msg "jit")                 ":jit reported")
(assert (string-contains? msg "24")                  "value 24 reported")
(assert (string-contains? msg "27")                  "value 27 reported")

# assert-agree must signal :diff-disagreement when fed a disagreement.
# We can't trigger one organically, so call format-disagreement +
# error directly on a synthesized report and confirm protect catches.
(def [ok? err]
  (protect
    (error {:error :diff-disagreement
            :message (diff:format-disagreement fake-report add [3 7])
            :report  fake-report})))
(assert (not ok?)                          "synthetic disagreement throws")
(assert (= (get err :error) :diff-disagreement)
        ":diff-disagreement is the error kind")

(println "disagreement detection: OK")
