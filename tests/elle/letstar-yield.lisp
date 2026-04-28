(elle/epoch 9)
# Reproducer for let*-ffi-signature bug (#673 remaining)
#
# TRIGGER: let* + yield + &named + large letrec env from import
#
# After let* resumes from a yield inside a function that:
# 1. Has &named parameters
# 2. Captures module-level defns (large letrec env from import-file)
# 3. Uses let* with a yielding thunk
#
# ...the env reconstruction corrupts captured variable types.
# With a large enough env, this segfaults.
# With a medium env, values become ffi-signature, list, or string types.
#
# The var/assign workaround avoids the bug entirely.
#
# Root cause: JIT yield-through-call LBox reconstruction
# (src/jit/suspend.rs, src/lir/emit/mod.rs)
#
# The letstar-helper.lisp module is a medium-size reproduction that
# triggers type corruption (not segfault). The actual telemetry.lisp
# module is large enough to trigger ffi-signature corruption.

# ── Workaround verification ──────────────────────────────────────
# Confirm that var/assign is not affected by this bug.

(defn yielding-thunk []
  (ev/sleep 0.001)
  42)

(defn time-var [thunk &named attributes]
  (def @start (clock/monotonic))
  (def @result (thunk))
  (def @elapsed (- (clock/monotonic) start))
  {:elapsed elapsed :result result :attrs attributes})

(let [r (time-var yielding-thunk :attributes {:a 1})]
  (assert (number? r:elapsed) "var: elapsed is number")
  (assert (= r:result 42) "var: result is 42")
  (assert (struct? r:attrs) "var: attrs is struct"))
(println "  var/assign workaround: PASS")

# ── Pure thunk baseline ──────────────────────────────────────────
# let* works fine when the thunk doesn't yield.

(defn time-let [thunk &named attributes]
  (let* [start (clock/monotonic)
         result (thunk)
         elapsed (- (clock/monotonic) start)]
    {:elapsed elapsed :result result :attrs attributes}))

(let [r (time-let (fn [] 42) :attributes {:a 1})]
  (assert (number? r:elapsed) "let* pure: elapsed is number")
  (assert (= r:result 42) "let* pure: result is 42")
  (assert (struct? r:attrs) "let* pure: attrs is struct"))
(println "  let* pure thunk: PASS")

# ── Known-buggy path (protected) ─────────────────────────────────
# When the bug is fixed, this will pass. Until then, it demonstrates
# the corruption. We protect to avoid crashing the test suite.

(let [[ok? result] (protect ((fn []
                               (let [r (time-let yielding-thunk
                                     :attributes {:a 1})]
                                 (assert (number? r:elapsed)
                                 "let* yield: elapsed is number")
                                 (assert (= r:result 42)
                                 "let* yield: result is 42")
                                 (assert (struct? r:attrs)
                                 "let* yield: attrs is struct")
                                 r))))]
  (if ok?
    (println "  let* yield (local defn): PASS — bug may be fixed!")
    (println "  let* yield (local defn): FAIL — " result)))

(println "")
(println "letstar-yield tests done.")
