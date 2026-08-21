(elle/epoch 12)
## tests/elle/fuel-jit-preempt.lisp
##
## Regression: when a fuel-limited fiber's body runs as JIT-compiled native
## code, the state it holds across a fuel-preemption yield must survive the
## refuel+resume — the compiled tier must preserve operand-stack values (an
## `apply` splice accumulator, a loop induction value) exactly as the bytecode
## interpreter does.
##
## The bytecode interpreter charges fuel at every branch/call opcode
## (`charge_fuel`, src/vm/dispatch/interp/opcodes.rs) and, on exhaustion, spills
## its operand stack into a SuspendedFrame that a later resume restores. A
## JIT-compiled body charges no fuel of its own; when a callee it invokes
## exhausts fuel and yields, the JIT side-exits through `elle_jit_yield` /
## `elle_jit_yield_through_call` (src/jit/suspend.rs), which spill the compiled
## frame's live params/locals/operands so the interpreter can resume the body.
## This test pins that that spill/restore preserves the value feeding an
## `apply` splice: after preemption the spliced argument must still be the
## computed list, never nil.
##
## Determinism (no compile-vs-execute race): under eager JIT a function is
## compiled on a background worker and only used once ready, so WHICH tier runs
## the fueled window is otherwise timing-dependent. Each scenario therefore
## warms its body once (submitting it to the worker) and then calls
## `(jit/rejections)`, whose `drain_jit_pending` blocks until the compile lands
## in the JIT cache. A fresh fiber over the SAME closure template then routes
## the fuel-limited run through JIT code deterministically. Under the bytecode
## tier (jit off) the warm/drain is inert and the body runs interpreted, so this
## file passes there unconditionally and pins the JIT tier specifically.
##
## Fibers use |:fuel :error| so a corruption-induced runtime error is caught by
## the fiber (recorded as its value) rather than propagating to the runner.

(def @failures 0)

(defn check [name ok]
  (if ok
    (println "  PASS:" name)
    (do
      (println "  FAIL:" name)
      (assign failures (+ failures 1)))))

# Warm BODY once (eager JIT submits it) then drain synchronously so its compiled
# code is in the JIT cache before the fueled run — the fiber over the same
# template then runs as JIT with no background-compile race.
(defn force-jit [body]
  (body)
  (jit/rejections)
  nil)

# ── Scenario 1: apply splice over a fuel-yielding argument call ─────────
#
# (apply + (range 100)) expands to a spliced call (+ (splice (range 100))).
# With fuel=500 the `range` call preempts mid-flight; after refuel the splice
# must still see the full list and sum to 4950. The JIT-tier bug surfaced here
# as either the fiber erroring ("splice: ... got nil") or a wrong sum.

(println "Scenario 1: apply splice across JIT preemption")
(defn apply-sum []
  (apply + (range 100)))
(force-jit apply-sum)
(let [f (fiber/new apply-sum |:fuel :error|)]
  (fiber/set-fuel f 500)
  (fiber/resume f)
  (check "fiber paused on fuel exhaustion" (= (fiber/status f) :paused))
  (fiber/set-fuel f 10000000)
  (fiber/resume f)
  (check "fiber completes after refuel (no splice nil error)"
         (= (fiber/status f) :dead))
  (check "correct sum (expected 4950)" (= (fiber/value f) 4950)))

# ── Scenario 2: apply concat with many chunks across preemption ─────────
#
# (apply concat (map ... (range 100))) over 100 two-byte arrays, fuel=500.
# After refuel the concatenation must yield 200 bytes.

(println "Scenario 2: apply concat across JIT preemption")
(defn apply-concat []
  (apply concat (map (fn [_] (bytes 65 66)) (range 100))))
(force-jit apply-concat)
(let [f (fiber/new apply-concat |:fuel :error|)]
  (fiber/set-fuel f 500)
  (fiber/resume f)
  (check "fiber paused on fuel exhaustion" (= (fiber/status f) :paused))
  (fiber/set-fuel f 10000000)
  (fiber/resume f)
  (check "fiber completes after refuel" (= (fiber/status f) :dead))
  (check "200 bytes from 100 two-byte chunks"
         (and (= (fiber/status f) :dead) (= (length (fiber/value f)) 200))))

# ── Scenario 3: fold accumulator across preemption (JIT-forced) ─────────
#
# The interpreter-tier fold-preemption fixes are pinned by fuel-apply-fold.lisp;
# this repeats the fold accumulator case with the body JIT-forced, so the
# compiled tier's spill/restore of the fold state is covered too.

(println "Scenario 3: fold accumulator across JIT preemption")
(defn fold-sum []
  (fold (fn [acc _] (+ acc 1)) 0 (range 100)))
(force-jit fold-sum)
(let [f (fiber/new fold-sum |:fuel :error|)]
  (fiber/set-fuel f 1000)
  (fiber/resume f)
  (check "fiber paused on fuel exhaustion" (= (fiber/status f) :paused))
  (fiber/set-fuel f 10000000)
  (fiber/resume f)
  (check "fiber completes after refuel" (= (fiber/status f) :dead))
  (check "accumulator preserved (expected 100)" (= (fiber/value f) 100)))

# ── Summary ───────────────────────────────────────────────────────────

(println)
(if (= failures 0)
  (println "All checks passed.")
  (do
    (println failures "check(s) failed.")
    (assert false "fuel-jit-preempt: regression tests failed")))
