# N-Queens Problem Solver in Janet
#
# This implementation correctly solves the N-Queens problem using recursive backtracking.
#
# ROOT CAUSE OF THE ORIGINAL BUG:
# The safe? function expects the queens array to be ordered with the most recently
# placed queen first (most recent row first), but the code was building the array
# in the opposite order by using array/push (which adds to the end).
#
# When checking if a column is safe, the algorithm iterates through queens[]:
#   - queens[0] is treated as being 1 row back (row-offset = 1)
#   - queens[1] is treated as being 2 rows back (row-offset = 2)
#   - etc.
#
# But array/push appends to the end, so:
#   - queens[0] was the FIRST queen placed (most rows ago)
#   - queens[1] was placed LATER
#
# This caused the algorithm to check diagonal threats incorrectly, rejecting all
# valid column placements and never reaching any solutions.
#
# THE FIX:
# Use array/insert with index 0 instead of array/push to insert new queens at
# the front of the array, maintaining the "most recent first" ordering that
# safe? expects.

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
      @[(array/slice queens)]
      # RECURSIVE CASE: try each column in current row
        (let [result @[]]
        (for col 0 n
          (when (safe? col queens)
            # This column is safe - place queen here
            (let [new-queens (array/insert (array/slice queens) 0 col)]
              # Recurse to place remaining queens
              # solve() returns an array of solutions from that subtree
              # array/concat returns the modified result array
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
