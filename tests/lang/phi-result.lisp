(elle/epoch 16)
# audited: 2026-11-17
# An `if` whose branches assign keeps its own value where it ends a sequence.
# docs/impl/hir.md
#
# Functionalization merges each assigned name after the `if` with a phi `let`.
# The counter-factual: a phi `let` that ended the sequence answered the merged
# name, not the `if`'s value.

(let [result (block (var @x 0)
               (if true
                 (begin
                   (assign x 1)
                   "yes")
                 (begin
                   (assign x 2)
                   "no")))]
  (assert (= result "yes")
          "phi merge last-in-begin: if result preserved (then branch)"))

(let [result (block (var @x 0)
               (if false
                 (begin
                   (assign x 1)
                   "yes")
                 (begin
                   (assign x 2)
                   "no")))]
  (assert (= result "no")
          "phi merge last-in-begin: if result preserved (else branch)"))

# A form after the `if` reads the merged value.
(begin
  (var @x 0)
  (if true (assign x 42) (assign x 99))
  (assert (= x 42) "phi merge with continuation: then branch value"))

(begin
  (var @x 0)
  (if false (assign x 42) (assign x 99))
  (assert (= x 99) "phi merge with continuation: else branch value"))

# `each` expands to a `match` whose arms bind mutable names, so an `if` around
# it takes the phi insertion too.
(defn f [x]
  (let [[a b] ["." x]]
    (if (= b "")
      @[]
      (let [acc @[]]
        (each i in (list 1 2 3)
          (push acc i))
        acc))))

(let [result (f "hello")]
  (assert (= (length result) 3) "phi merge with each: array has 3 elements")
  (assert (= (get result 0) 1) "phi merge with each: first element")
  (assert (= (get result 1) 2) "phi merge with each: second element")
  (assert (= (get result 2) 3) "phi merge with each: third element"))

(println "phi-result: all tests passed")
