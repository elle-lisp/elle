(elle/epoch 12)
## audited: 2026-09-29
## jit/rejections names each function the JIT refused, with its reason and
## call count.
## docs/impl/jit.md
##
## A build without the JIT compiles and refuses nothing, and `(vm/config :jit)`
## reads nil there, so each assertion is gated on a live JIT. The top level
## stays flat so that the call counts the last assertion depends on are the
## ones the running policy sees.
(def @jit-on? (not (nil? (vm/config :jit))))

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

## Each rejection is a struct with :name, :reason, :calls. Without a JIT
## `rejections` is empty and `first` would fault, so take it only when the JIT
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
