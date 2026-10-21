(elle/epoch 15)
# audited: 2026-10-21
# A value read out of a pair stays live for as long as each name it reaches, through a wrapper, a branch, a pattern or a second read.
#
# docs/impl/region/borrow.md
#
# The sidecar arms guardfree, keeps the JIT off and turns the graft off. Each
# face is written as the graft would leave it, so the shape reaches the region
# solver as written, and a stale read faults at its deref.
#
# Every face hands the result of a `%first` or `%rest` opcode read on to a
# name, or to a second read, and reads it after the pair's own last mention.
# The opcode raises no count, so the pair's region is the borrowed value's only
# protection.
#
# The counter-factual: with the name holding nothing, the pair would be released
# at the read's own consumer, and the reader would deref a freed page. A plain `let`
# around the read can hide that, because its body happens to outlast the reader;
# the `begin` wrapper and the second read do not.
#
# DISTINCT FROM tests/impl/region-container-read-borrow-uaf.lisp, whose opcode
# read sits in operand position with its result unnamed.

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

# The second read is an operand, so no name carries its result.
(defn operand-rest [i]
  (let [xs (%pair i (%pair i (%pair (string "s" i) ())))]
    (let [p (%rest xs)]
      (when (%not (%pair? p))
        (error {:error :type-error :message "operand-rest: pair expected"}))
      (length (first (%rest p))))))

# Each arm of a branch reads out of its own pair.
(defn arm-first [i]
  (let [xs (%pair (string "s" i) ())
        ys (%pair (string "t" i) ())]
    (let [t (let [u (if (< i 0)
                      (begin
                        nil
                        (%first ys))
                      (begin
                        nil
                        (%first xs)))]
              u)]
      (length t))))

# A `break` hands the read to its block.
(defn break-first [i]
  (let [xs (%pair (string "s" i) ())]
    (let [t (let [p (begin
                      nil
                      (block :b
                        (break :b (%first xs))))]
              p)]
      (length t))))

# A pattern binds a name out of a borrowed scrutinee.
(defn match-first [i]
  (let [xs (%pair (%pair (string "s" i) ()) ())]
    (let [t (%first xs)]
      (let [y (match t
                (h . tl) h
                _ nil)]
        (length y)))))

# A destructure of a borrowed value reads its fields after the value's own
# last read.
(defn destructure-first [i]
  (let [xs (%pair (%pair (string "s" i) (%pair i ())) ())]
    (let [t (let [p (begin
                      nil
                      (%first xs))]
              p)]
      (let [(a b) t]
        (length a)))))

# A pattern whose value is the read itself, with no name read. The read's own
# last use narrows to the read, and the extraction comes after it.
(defn destructure-read [i]
  (let [xs (%pair (%pair (string "s" i) (%pair i ())) ())]
    (def (a b) (%first xs))
    (length (string "s" i))))

# A closure captures the pair, so the ownership forest may hand the pair to the
# closure's subtree, which frees it with the closure. The borrow is read after
# the closure's last call.
(defn capture-first [i]
  (let [xs (%pair (string "s" i) ())
        f (fn [] (%pair? xs))
        t (let [p (begin
                    nil
                    (%first xs))]
            p)]
    (f)
    (length t)))

(defn churn [n]
  (def @i 0)
  (while (< i n)
    (def want (length (string "s" i)))
    (assert (= (begin-rest i) want) "begin-rest reads its borrow")
    (assert (= (begin-first i) want) "begin-first reads its borrow")
    (assert (= (tail-first i) want) "tail-first reads its borrow")
    (assert (= (chain-rest i) want) "chain-rest reads its borrow")
    (assert (= (chain-first i) want) "chain-first reads its borrow")
    (assert (= (operand-rest i) want) "operand-rest reads its borrow")
    (assert (= (arm-first i) want) "arm-first reads its borrow")
    (assert (= (break-first i) want) "break-first reads its borrow")
    (assert (= (match-first i) want) "match-first reads its borrow")
    (assert (= (destructure-first i) want) "destructure-first reads its borrow")
    (assert (= (destructure-read i) want) "destructure-read extracts its borrow")
    (assert (= (capture-first i) want) "capture-first reads its borrow")
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
