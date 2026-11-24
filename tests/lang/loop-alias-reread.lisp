(elle/epoch 19)
# audited: 2026-11-24
# A loop that names a parameter again on each pass reads the whole parameter on every pass.
# docs/impl/region/lastuse.md
#
# `(let [s b] …)` inside the outer loop is a read of `b` on each pass, so its
# last use is the outer loop. The counter-factual: when `s`'s own reads end that
# last use inside the loop, the parameter's slot is released after the first
# pass, the later passes read nil, and the sum comes out 30.

(defn start [s]
  (if (list? s) s 0))

(defn sum-thrice [b]
  (def @n 0)
  (def @i 0)
  (while (< i 3)
    (let [s b]
      (let [@at (start s)]
        (while (pair? at)
          (assign n (+ n (first at)))
          (assign at (rest at)))))
    (assign i (+ i 1)))
  n)

(assert (= (sum-thrice (list 10 20)) 90)
        "each of the three passes walks the whole list")

(defn start-by-type [s]
  (match (type-of s)
    :list s
    _ 0))

(defn sum-thrice-by-type [b]
  (def @n 0)
  (def @i 0)
  (while (< i 3)
    (let [s b]
      (let [@at (start-by-type s)]
        (while (pair? at)
          (assign n (+ n (first at)))
          (assign at (rest at)))))
    (assign i (+ i 1)))
  n)

(assert (= (sum-thrice-by-type (list 10 20)) 90)
        "a type-of dispatch in the callee walks the whole list on each pass")

(defn count-thrice [b]
  (def @n 0)
  (def @i 0)
  (while (< i 3)
    (let [s b]
      (assign n (+ n (length s))))
    (assign i (+ i 1)))
  n)

(assert (= (count-thrice (list 10 20)) 6)
        "an alias read directly on each pass sees the whole list")

## Two callers pass a list and an array, so the loops dispatch at run time.
## The counter-factual: an inner loop that frees the parameter it walks after
## the first outer pass sums the first row alone.
(defn sum-products [xs ys]
  (var n 0)
  (each x in xs
    (each y in ys
      (assign n (+ n (* x y)))))
  n)

(assert (= (sum-products [1 2 3] (list 10 20)) 180)
        "each: an inner loop walks a list parameter on every outer pass")
(assert (= (sum-products (list 1 2 3) [10 20]) 180)
        "each: an inner loop walks an array parameter on every outer pass")
