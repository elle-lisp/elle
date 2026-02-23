; N-Queens Problem Solver in Elle
;
; This implementation solves the N-Queens problem using recursive backtracking.
; Tests Elle's handling of recursion, list operations, and result accumulation.

(var check-safe-helper
  (fn (col remaining row-offset)
    (if (empty? remaining)
      #t
      (let ((placed-col (first remaining)))
        (if (or (= col placed-col)
                (= row-offset (abs (- col placed-col))))
          #f
          (check-safe-helper col (rest remaining) (+ row-offset 1)))))))

(var safe?
  (fn (col queens)
    "Check if column col is safe given previously placed queens.
     queens = list of columns from previous rows, most recent first."
    (check-safe-helper col queens 1)))

(var try-cols-helper
  (fn (n col queens row)
    "Helper to try all columns for a given row."
    (if (= col n)
      (list)
      (if (safe? col queens)
        ; This column is safe - place queen here
        (let ((new-queens (cons col queens)))
          ; Recurse to place remaining queens
          ; solve-helper returns a list of solutions from that subtree
          ; append combines solutions from this branch with remaining branches
          (append (solve-helper n (+ row 1) new-queens)
                  (try-cols-helper n (+ col 1) queens row)))
        ; Column not safe, try next column
        (try-cols-helper n (+ col 1) queens row)))))

(var solve-helper
  (fn (n row queens)
    "Recursive backtracking solver.
     Base case (row == n): All queens placed -> one solution found
     Recursive case: Try each column, recurse, accumulate solutions"
    (if (= row n)
      ; BASE CASE: successfully placed all n queens
      (list (reverse queens))
      ; RECURSIVE CASE: try each column in current row
      (try-cols-helper n 0 queens row))))

(var solve-nqueens
  (fn (n)
    "Return list of solutions. Each solution is a list of column positions."
    (solve-helper n 0 (list))))

(var benchmark
  (fn (n)
    (display "Solving N-Queens for N=")
    (display n)
    (display "... ")
    (let ((solutions (solve-nqueens n)))
      (display "Found ")
      (display (length solutions))
      (display " solution(s)")
      (newline))))

(display "=== N-Queens Solver (Elle) ===\n\n")
(benchmark 12)
(display "=== Complete ===\n")
