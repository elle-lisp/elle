# N-Queens Problem Solver in Janet
# 
# *** BUG: This implementation finds 0 solutions instead of the correct count ***
# Expected: N=4 -> 2 solutions, N=8 -> 92 solutions
# Actual: All return 0 solutions
#
# The algorithm mirrors the working Chez Scheme and SBCL versions.
# The safe? function works correctly when tested in isolation.
# The issue appears to be in how recursive solutions are accumulated via array/concat.
#
# SUSPECTED ROOT CAUSE:
# When solve() is called recursively, it returns an array of solutions.
# We try to append these to our 'result' array using array/concat.
# However, result always stays empty, suggesting:
#   1. array/concat may not be working as expected
#   2. The 'result' variable may be shadowed or scoped incorrectly
#   3. The for loop + when block may not properly accumulate into result
#   4. There could be a subtle issue with array mutation and closure capture
#
# DEBUGGING TIPS:
# - Add (printf "concat result: %v with solutions: %v\n" result solutions)
#   before each array/concat call to trace what's happening
# - Verify array/concat is actually modifying result by checking its length
# - Try implementing an iterative solver that builds solutions without recursion
# - Compare with a working recursive accumulator in pure Janet to isolate the issue

(defn safe? [col queens]
  "Check if column col is safe given previously placed queens.
   queens = array of columns from previous rows, most recent first."
  (var safe true)
  (var row-offset 1)
  (var idx 0)
  (while (and (< idx (length queens)) safe)
    (let [placed-col (queens idx)]
      (when (or (= col placed-col)
                (= row-offset (math/abs (- col placed-col))))
        (set safe false)))
    (set row-offset (+ row-offset 1))
    (set idx (+ idx 1)))
  safe)

(defn solve-nqueens [n]
  "Return array of solutions. Each solution is an array of column positions."
  (defn solve [row queens]
    "Recursive backtracking solver.
     
     Base case (row == n): All queens placed -> one solution found
     Recursive case: Try each column, recurse, accumulate solutions"
    (if (= row n)
      # BASE CASE: successfully placed all n queens
      (let [result @[]]
        (array/push result (array/slice queens))
        result)
      # RECURSIVE CASE: try each column in current row
      (let [result @[]]
        (for col 0 n
          (when (safe? col queens)
            # This column is safe - place queen here
            (let [new-queens (array/push (array/slice queens) col)]
              # Recurse to place remaining queens
              # solve() returns an array of solutions from that subtree
              # We concat those solutions into our result array
              # 
              # THIS IS WHERE THE BUG LIKELY IS:
              # After this line, result should contain accumulated solutions
              # but it appears to stay empty no matter how many times this runs
              (array/concat result (solve (+ row 1) new-queens)))))
        result)))
  (solve 0 @[]))

(defn benchmark [n]
  (printf "Solving N-Queens for N=%d... " n)
  (let [solutions (solve-nqueens n)]
    (printf "Found %d solution(s)\n" (length solutions))))

(print "=== N-Queens Solver (Janet) ===\n\n")
(benchmark 4)
(benchmark 8)
(benchmark 10)
(benchmark 12)
(print "=== Complete ===\n")
