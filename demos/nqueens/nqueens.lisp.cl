;;; N-Queens Problem Solver in Common Lisp
;;;
;;; This implementation solves the N-Queens problem using recursive backtracking.
;;; Tests recursion, list operations, and result accumulation.

(defun check-safe-helper (col remaining row-offset)
  (if (null remaining)
    t
    (let ((placed-col (car remaining))
          (rest-queens (cdr remaining)))
      (if (or (= col placed-col)
              (= row-offset (abs (- col placed-col))))
        nil
        (check-safe-helper col rest-queens (+ row-offset 1))))))

(defun safe? (col queens)
  "Check if column col is safe given previously placed queens.
   queens = list of columns from previous rows, most recent first."
  (check-safe-helper col queens 1))

(defun try-cols-helper (n col queens row)
  "Helper to try all columns for a given row."
  (if (= col n)
    nil
    (if (safe? col queens)
      ; This column is safe - place queen here
      (let ((new-queens (cons col queens)))
        ; Recurse to place remaining queens
        ; solve-helper returns a list of solutions from that subtree
        ; append combines solutions from this branch with remaining branches
        (append (solve-helper n (+ row 1) new-queens)
                (try-cols-helper n (+ col 1) queens row)))
      ; Column not safe, try next column
      (try-cols-helper n (+ col 1) queens row))))

(defun solve-helper (n row queens)
  "Recursive backtracking solver.
   Base case (row == n): All queens placed -> one solution found
   Recursive case: Try each column, recurse, accumulate solutions"
  (if (= row n)
    ; BASE CASE: successfully placed all n queens
    (list (reverse queens))
    ; RECURSIVE CASE: try each column in current row
    (try-cols-helper n 0 queens row)))

(defun solve-nqueens (n)
  "Return list of solutions. Each solution is a list of column positions."
  (solve-helper n 0 nil))

(defun benchmark (n)
  (format t "Solving N-Queens for N=~D... " n)
  (let ((solutions (solve-nqueens n)))
    (format t "Found ~D solution(s)~%" (length solutions))))

(format t "=== N-Queens Solver (Common Lisp) ===~%~%")
(benchmark 4)
(benchmark 8)
(benchmark 10)
(benchmark 12)
(format t "=== Complete ===~%")
