(elle/epoch 6)

# N-Queens Problem Solver in Elle
#
# Solves N-Queens via recursive backtracking with cons-list board state.
# All recursion through top-level defn — no closure allocation in the
# hot path.  The board is a cons list: (cons col queens) to place,
# (rest queens) to backtrack.  This is the natural representation for
# backtracking search.

(defn check-safe [col remaining offset]
  "Walk placed queens checking column conflicts and diagonals."
  (if (empty? remaining)
    true
    (if (or (= col (first remaining))
            (= offset (abs (- col (first remaining)))))
      false
      (check-safe col (rest remaining) (+ offset 1)))))

(defn safe? [col queens]
  "True when col doesn't conflict with any previously placed queen."
  (check-safe col queens 1))

(defn try-col [n col queens row count]
  "Try columns col..n-1 for the given row, returning updated count."
  (if (= col n)
    count
    (try-col n (+ col 1) queens row
      (if (safe? col queens)
        (search n (+ row 1) (cons col queens) count)
        count))))

(defn search [n row queens count]
  "Recursive backtracking from row, returning total solution count."
  (if (= row n)
    (+ count 1)
    (try-col n 0 queens row count)))

(defn solve [n]
  "Count all N-Queens solutions for an n*n board."
  (search n 0 (list) 0))

(defn benchmark [n]
  (println "Solving N-Queens for N=" n "... Found " (solve n) " solution(s)"))

(println "=== N-Queens Solver (Elle) ===")
(println)
(benchmark 12)
(println "=== Complete ===")
