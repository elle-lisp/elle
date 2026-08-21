(elle/epoch 12)
## Whole-file per-policy execution (process-whole / whole-file-policies).
##
## A legacy multi-form file is run once under the :off JIT policy (recorded "vm")
## and once under :eager (recorded "jit") — the smoke-vm + smoke-jit split folded
## into one run. This fixture asserts the file actually OBSERVES that policy via
## (vm/config :jit) — never the default :adaptive. It is the counter-factual for a
## no-op policy set (e.g. the `(put (vm/config) :jit …)` rewrite silently not
## firing): under that bug the file sees :adaptive and FAILS here on both tiers,
## rather than running under the wrong policy behind a correct-looking label.
(def pol (vm/config :jit))
(assert (or (= pol :off) (= pol :eager))
        (string "expected a per-policy run (:off or :eager), got " pol))
