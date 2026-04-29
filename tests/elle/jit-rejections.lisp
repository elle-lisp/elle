(elle/epoch 9)
## jit/rejections — test JIT rejection tracking

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
(assert (> (length rejections) initial-count)
        "expected new rejection from has-eval")

## Each rejection is a struct with :name, :reason, :calls
(def @r (first rejections))
(assert (has-key? r :name) "rejection has :name")
(assert (has-key? r :reason) "rejection has :reason")
(assert (has-key? r :calls) "rejection has :calls")
(assert (string? (get r :name)) ":name is a string")

## A pure hot function should NOT appear in rejections
(defn pure-hot (n)
  (if (<= n 0) 0 (pure-hot (- n 1))))
(pure-hot 20)

## Rejections should not have grown beyond has-eval
(assert (= (length (jit/rejections)) (length rejections))
        "pure hot function does not add to rejections")
