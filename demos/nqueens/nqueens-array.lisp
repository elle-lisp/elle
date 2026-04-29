(elle/epoch 9)

# N-Queens Problem Solver — mutable array version
#
# Uses a single mutable array as a stack for backtracking.
# Push to place a queen, pop to backtrack. Safety check
# iterates array indices directly.

(defn check-safe [col queens idx row]
  "Check col against queens[idx..row)."
  (if (= idx row)
    true
    (let* [placed-col (get queens idx)
           row-dist (- row idx)]
      (if (or (= col placed-col) (= row-dist (abs (- col placed-col))))
        false
        (check-safe col queens (+ idx 1) row)))))

(defn safe? [col queens row]
  "Check if column col is safe given row queens already placed."
  (check-safe col queens 0 row))

(defn try-cols [n col queens row]
  "Try columns col..n for the given row."
  (if (= col n)
    (list)
    (if (safe? col queens row)
      (begin
        (push queens col)
        (let* [solutions (solve n (+ row 1) queens)]
          (pop queens)
          (concat solutions (try-cols n (+ col 1) queens row))))
      (try-cols n (+ col 1) queens row))))

(defn solve [n row queens]
  "Recursive backtracking. row = number of queens placed so far."
  (if (= row n)
    (list (list ;queens))
    (try-cols n 0 queens row)))

(defn solve-nqueens [n]
  (solve n 0 @[]))

(defn benchmark [n]
  (let* [solutions (solve-nqueens n)]
    (println "Solving N-Queens for N=" n "... Found " (length solutions)
             " solution(s)")))

(println "=== N-Queens Solver (Elle, array) ===")
(println)
(benchmark 12)
(println "=== Complete ===")
