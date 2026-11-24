(elle/epoch 18)
# audited: 2026-11-23
# A loop that names a parameter again on each pass reads the whole parameter on every pass.
# docs/impl/region/lastuse.md
#
# `(let [s b] …)` inside the outer loop is a read of `b` on each pass. Its last
# use must be the outer loop. When `s`'s own reads ended it inside the loop, the
# parameter's slot was released after the first pass, and the later passes read
# nil (#1558). The sum then came out 30, the first pass alone.

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
