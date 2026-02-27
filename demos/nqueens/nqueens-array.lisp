# N-Queens Problem Solver in Elle — mutable array version
#
# Uses a single mutable array as a stack for backtracking.
# Push to place a queen, pop to backtrack. Safety check
# iterates array indices directly.

(var check-safe-helper
  (fn (col queens idx row)
    "Check col against queens[idx..row). row = current number of placed queens."
    (if (= idx row)
      true
      (let ((placed-col (array/ref queens idx)))
        (let ((row-dist (- row idx)))
          (if (or (= col placed-col)
                  (= row-dist (abs (- col placed-col))))
            false
            (check-safe-helper col queens (+ idx 1) row)))))))

(var safe?
  (fn (col queens row)
    "Check if column col is safe given row queens already placed."
    (check-safe-helper col queens 0 row)))

(var array->list
  (fn (arr n)
    "Convert first n elements of array to list."
    (var helper
      (fn (idx acc)
        (if (= idx 0)
          acc
          (let ((i (- idx 1)))
            (helper i (cons (array/ref arr i) acc))))))
    (helper n (list))))

(var try-cols
  (fn (n col queens row)
    "Try columns col..n for the given row."
    (if (= col n)
      (list)
      (if (safe? col queens row)
        (let ((_ (array/push! queens col)))
          (let ((solutions (solve n (+ row 1) queens)))
            (let ((_ (array/pop! queens)))
              (append solutions
                      (try-cols n (+ col 1) queens row)))))
        (try-cols n (+ col 1) queens row)))))

(var solve
  (fn (n row queens)
    "Recursive backtracking. row = number of queens placed so far."
    (if (= row n)
      (list (array->list queens n))
      (try-cols n 0 queens row))))

(var solve-nqueens
  (fn (n)
    (solve n 0 (array))))

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

(display "=== N-Queens Solver (Elle, array) ===\n\n")
(benchmark 12)
(display "=== Complete ===\n")
