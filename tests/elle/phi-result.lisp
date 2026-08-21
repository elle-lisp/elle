(elle/epoch 12)
## If-result preservation across phi-merge
##
## Migrated from src/hir/functionalize.rs Rust tests.
## When an `if` containing assigns is the last expression in a begin,
## the phi-lets must not discard the if's result value.

# if_result_preserved_with_phi_merge_last_in_begin
(let [result (begin
               (var @x 0)
               (if true
                 (begin
                   (assign x 1)
                   "yes")
                 (begin
                   (assign x 2)
                   "no")))]
  (assert (= result "yes")
          "phi merge last-in-begin: if result preserved (then branch)"))

# if_result_preserved_with_phi_merge_else_branch
(let [result (begin
               (var @x 0)
               (if false
                 (begin
                   (assign x 1)
                   "yes")
                 (begin
                   (assign x 2)
                   "no")))]
  (assert (= result "no")
          "phi merge last-in-begin: if result preserved (else branch)"))

# if with continuation: phi-lets correctly merge assigned values
(begin
  (var @x 0)
  (if true (assign x 42) (assign x 99))
  (assert (= x 42) "phi merge with continuation: then branch value"))

(begin
  (var @x 0)
  (if false (assign x 42) (assign x 99))
  (assert (= x 99) "phi merge with continuation: else branch value"))

# if_result_preserved_with_each_loop
# The original bug: `each` expands to a match with mutable defines
# inside branches, triggering phi insertion that discards the
# if's return value.
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
