(elle/epoch 12)
## jit/rejections — test JIT rejection tracking

## JIT rejection tracking is only observable when the JIT is actually enabled.
## Under `elle test`'s bytecode (vm) tier the thunk runs via
## (compile/run-on :bytecode …), which forces JitPolicy::Off — so no function is
## ever JIT-compiled or rejected and (jit/rejections) stays empty. That is
## correct for a pure-bytecode run, not a bug, so gate each rejection assertion
## on the live policy. The flat top-level
## structure is preserved exactly so the adaptive-JIT call counts that the last
## assertion depends on are unperturbed; a direct `make smoke` run (adaptive JIT)
## still exercises every assertion.
(def @jit-on? (not= (vm/config :jit) :off))

## Record initial rejections (stdlib functions with SuspendingCall may be rejected)
(def @initial-count (length (jit/rejections)))

## A function containing eval gets rejected when hot.
(defn has-eval (n)
  (if (<= n 0)
    0
    (+ (eval '1) (has-eval (- n 1)))))

(has-eval 20)

(def @rejections (jit/rejections))

## At least one new rejection recorded
(assert (or (not jit-on?) (> (length rejections) initial-count))
        "expected new rejection from has-eval")

## Each rejection is a struct with :name, :reason, :calls. Under the bytecode
## tier `rejections` is empty and `first` would fault, so only take it when JIT
## is live; the (or (not jit-on?) …) gates then skip the field access.
(def @r (if jit-on? (first rejections) nil))
(assert (or (not jit-on?) (has-key? r :name)) "rejection has :name")
(assert (or (not jit-on?) (has-key? r :reason)) "rejection has :reason")
(assert (or (not jit-on?) (has-key? r :calls)) "rejection has :calls")
(assert (or (not jit-on?) (string? (get r :name))) ":name is a string")

## A pure hot function should NOT appear in rejections
(defn pure-hot (n)
  (if (<= n 0) 0 (pure-hot (- n 1))))
(pure-hot 20)

## Rejections should not have grown beyond has-eval
(assert (or (not jit-on?) (= (length (jit/rejections)) (length rejections)))
        "pure hot function does not add to rejections")
