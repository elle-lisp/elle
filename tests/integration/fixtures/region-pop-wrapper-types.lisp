(elle/epoch 12)
# tests/integration/fixtures/region-pop-wrapper-types.lisp
#
# GREEN guardfree guard for the stdlib `pop` wrapper across its three mutable
# container types — the counterfactual that keeps the pop moves-out fix honest.
# `pop` is a `(match (type-of coll) …)` wrapper over the monomorphic funnels
# `%pop`/`%pop-string`/`%pop-bytes`. Two things must stay balanced:
#
#   * The @array arm (`%pop`) moves out a PRE-EXISTING heap element; its redundant
#     tail ReturnValue retain is SUPPRESSED (`moves_out_release_sites`) and an element
#     adopted into an Owned container is EXTRACTED (`extract_owned_region`). Suppress
#     too much and the element over-frees; suppress too little and it leaks.
#   * The @string / @bytes arms return a FRESH grapheme / an immediate byte, which are
#     NOT suppressed (they need their tail retain). Suppressing them here would
#     over-free the returned grapheme — the Q1 hazard this guard pins.
#
# The wrapper's owned-param container also strands across the match arms (the F1b
# container strand); the container compensation frees it per-arm. All three drivers
# READ the popped result AFTER return and prime id churn first, so a stale-region
# deref of any over-freed result faults under guardfree. Runs clean today; a
# regression that unbalances any arm SIGSEGVs here.

(defn stmt-run [thunk]
  (fn [b]
    (def @i 0)
    (while (< i b)
      (thunk)
      (assign i (+ i 1)))))

# @array: pop a heap element (pushed through the funnel) and READ it after return.
(defn pop-array []
  (let [a @[]]
    (push a (list 1 2))
    (pop a)))
(defn use-array []
  (let [e (pop-array)]
    (first e)))

# @string: pop a FRESH grapheme and build it into another string (a read after return).
(defn pop-string []
  (let [s @"abcde"]
    (push s "z")
    (pop s)))
(defn use-string []
  (let [g (pop-string)]
    (string "got-" g)))

# @bytes: pop an immediate byte (no region — retain/decref no-op either way).
(defn pop-bytes []
  (let [b (thaw (bytes 1 2 3))]
    (pop b)))

((stmt-run (fn [] (use-array))) 1000)
((stmt-run (fn [] (use-array))) 1000)
((stmt-run (fn [] (use-string))) 1000)
((stmt-run (fn [] (use-string))) 1000)
((stmt-run (fn [] (pop-bytes))) 1000)

(println "region-pop-wrapper-types: ok")
