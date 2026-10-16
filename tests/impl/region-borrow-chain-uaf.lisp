(elle/epoch 15)
# audited: 2026-10-15
# A value read out of a pair stays live for as long as each name it is bound to, through a wrapper and through a second read.
# docs/impl/region/rules.md
#
# The sidecar arms guardfree, keeps the JIT off and turns the graft off: each
# face is written as the graft would leave it, so the shape reaches the region
# solver as written, and a stale read faults at its deref.
#
# Every face binds the result of a `%first` or `%rest` opcode read and reads the
# name after the pair's own last mention. The opcode raises no count, so the
# pair's region is the borrowed value's only protection.
#
# The counter-factual: with the bound name holding no region, the pair is
# released at the binder, and the reader derefs a freed page. A plain `let`
# around the read can hide that, because its body happens to outlast the reader;
# the `begin` wrapper and the second read do not.
#
# DISTINCT FROM region-container-read-borrow-uaf.lisp, whose opcode read sits in
# operand position with its result unnamed.

# The read is the tail of a statement wrapper around the init.
(defn begin-rest [i]
  (let [xs (%pair i (%pair (string "s" i) ()))]
    (let [t (let [p (begin
                      nil
                      (%rest xs))]
              p)]
      (length (first t)))))

(defn begin-first [i]
  (let [xs (%pair (string "s" i) ())]
    (let [t (let [p (begin
                      nil
                      (%first xs))]
              p)]
      (length t))))

# The reader is a tail call. The lowerer carries a release ahead of a tail call
# that names none of the region's holders, so `t` must name the pair's region.
(defn tail-first [i]
  (let [xs (%pair (string "s" i) ())]
    (let [t (begin
              nil
              (%first xs))]
      (length t))))

# A borrow out of a borrow: `q` reads out of `p`, which reads out of `xs`.
(defn chain-rest [i]
  (let [xs (%pair i (%pair i (%pair (string "s" i) ())))]
    (let [p (%rest xs)]
      (when (%not (%pair? p))
        (error {:error :type-error :message "chain-rest: pair expected"}))
      (let [q (%rest p)]
        (length (first q))))))

(defn chain-first [i]
  (let [xs (%pair (%pair (string "s" i) ()) ())]
    (let [p (%first xs)]
      (when (%not (%pair? p))
        (error {:error :type-error :message "chain-first: pair expected"}))
      (let [q (%first p)]
        (length q)))))

(defn churn [n]
  (def @i 0)
  (while (< i n)
    (def want (length (string "s" i)))
    (assert (= (begin-rest i) want) "begin-rest reads its borrow")
    (assert (= (begin-first i) want) "begin-first reads its borrow")
    (assert (= (tail-first i) want) "tail-first reads its borrow")
    (assert (= (chain-rest i) want) "chain-rest reads its borrow")
    (assert (= (chain-first i) want) "chain-first reads its borrow")
    (assign i (+ i 1))))

# Churn region ids so a freed page is recycled onto a live region.
(churn 500)

# The borrow extends the pair's lifetime to the reader and no further, so the
# region count stays flat across a second window.
(def r0 (arena/region-count))
(churn 500)
(def growth (- (arena/region-count) r0))
(assert (< growth 50)
        (string "chained borrow leaked " growth " regions over 500 ops"))

(println "region-borrow-chain-uaf: ok")
