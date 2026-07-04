(elle/epoch 12)
# Counterfactual for the native-call-result region leak.
#
# `dispatch_native_call` mints a fresh runtime region per native call and
# allocates the result into it. The caller balances that with a
# `DecrefValueRegion` at the result's decref_point. A *fresh* result (one
# allocated into this call's own region) therefore needs NO escape-incref —
# its single owning ref is exactly what the caller's decref consumes.
#
# The historical code compared the *runtime* result region against the
# *static* call slot (`result_region.get() == region_id`), a cross-id-space
# compare that is never true for a freshly minted runtime id. So every fresh
# native result got a spurious `incref_for_escape`, leaving its region stuck
# at rc=1 after the caller's decref — never freed. One leaked region per
# native call.
#
# This test drops every result, so a correct runtime frees each call's region
# immediately. Region-count growth must stay bounded and must NOT scale with
# the number of calls.

# A fresh-allocating native whose result is discarded each iteration.
(defn churn [n]
  (def before (arena/region-count))
  (def @i 0)
  (while (%lt i n)
    (string "x" "y")
    (assign i (%add i 1)))
  (%sub (arena/region-count) before))

(let [d100 (churn 100)
      d1000 (churn 1000)]
  (assert (%lt d100 20)
          (string "native-result region leak at n=100: delta=" d100))
  (assert (%lt d1000 20)
          (string "native-result region leak at n=1000: delta=" d1000)))
