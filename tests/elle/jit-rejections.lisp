## jit/rejections — test JIT rejection tracking

## Before any hot functions, jit/rejections returns empty list
(assert (= (jit/rejections) ()) "jit/rejections empty on fresh VM")

## A function containing MakeClosure gets rejected when hot.
(defn has-closure (n)
  (let ((f (fn (x) (+ x 1))))
    (if (<= n 0) 0
      (+ (f n) (has-closure (- n 1))))))

(has-closure 20)

(var rejections (jit/rejections))

## At least one rejection recorded
(assert (>= (length rejections) 1)
  "expected at least 1 rejection")

## Each rejection is a struct with :name, :reason, :calls
(var r (first rejections))
(assert (has-key? r :name) "rejection has :name")
(assert (has-key? r :reason) "rejection has :reason")
(assert (has-key? r :calls) "rejection has :calls")

## Reason mentions MakeClosure
(assert (string/contains? (get r :reason) "MakeClosure")
  "reason mentions MakeClosure")

## Call count is at least the JIT threshold (10)
(assert (>= (get r :calls) 10) ":calls >= JIT threshold")

## Name is a string
(assert (string? (get r :name)) ":name is a string")

## A pure hot function should NOT appear in rejections
(defn pure-hot (n)
  (if (<= n 0) 0 (pure-hot (- n 1))))
(pure-hot 20)

## Rejections should not have grown (pure-hot compiles successfully)
(assert (= (length (jit/rejections)) (length rejections))
  "pure hot function does not add to rejections")
