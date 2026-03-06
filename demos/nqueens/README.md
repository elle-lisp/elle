# N-Queens Problem Solver

## What This Demo Does

This demo solves the classic N-Queens problem: place N queens on an N×N chessboard such that no two queens attack each other. It uses recursive backtracking to explore all valid placements and counts the total number of solutions.

For N=12, there are exactly 14,200 valid solutions.

**Key features demonstrated:**
- Recursive backtracking algorithm
- List operations (`cons`, `first`, `rest`, `append`, `reverse`)
- Predicate functions (`safe?`)
- Accumulation of results through recursion

## How It Works

### The Problem

Two queens attack each other if they share:
- The same column
- The same row
- The same diagonal

The algorithm places queens row by row, left to right. For each row, it tries each column and checks if that placement is safe given the queens already placed in previous rows.

### The Algorithm

The solver uses three mutually recursive functions:

**`check-safe-helper`** — Validates a column placement
```janet
(defn check-safe-helper (col remaining row-offset)
  (if (empty? remaining)
    true
    (let* ((placed-col (first remaining)))
      (if (or (= col placed-col)
              (= row-offset (abs (- col placed-col))))
        false
        (check-safe-helper col (rest remaining) (+ row-offset 1))))))
```

This walks through previously placed queens (stored as a list of column indices) and checks:
- Same column? → Not safe
- Same diagonal? (row distance = column distance) → Not safe
- Otherwise, check the next queen

**`safe?`** — Public interface to check-safe-helper
```janet
(defn safe? (col queens)
  (check-safe-helper col queens 1))
```

**`try-cols-helper`** — Tries each column in the current row
```janet
(defn try-cols-helper (n col queens row)
  (if (= col n)
    (list)
    (if (safe? col queens)
      (let* ((new-queens (cons col queens)))
        (append (solve-helper n (+ row 1) new-queens)
                (try-cols-helper n (+ col 1) queens row)))
      (try-cols-helper n (+ col 1) queens row))))
```

If the current column is safe, place a queen there and recurse to the next row. Then try the next column. If not safe, skip to the next column.

**`solve-helper`** — Main recursive solver
```janet
(defn solve-helper (n row queens)
  (if (= row n)
    (list (reverse queens))
    (try-cols-helper n 0 queens row)))
```

Base case: all N queens placed → return the solution.
Recursive case: try all columns in the current row.

### Data Structure

Queens are stored as a list of column indices, most recent first:
- `(list)` — no queens placed yet
- `(list 0)` — one queen at column 0
- `(list 1 0)` — queens at columns 1 and 0 (row 1 and row 0)

When a solution is found, the list is reversed to get the canonical order (row 0 to row N-1).

## Sample Output

```
=== N-Queens Solver (Elle) ===

Solving N-Queens for N=12... Found 14200 solution(s)
=== Complete ===
```

## Elle Idioms Used

- **`defn`** — Function definition macro. Cleaner than `(def name (fn ...))`
- **`let*`** — Sequential bindings. Each binding can reference previous ones
- **`cons`** — Prepend an element to a list
- **`first` / `rest`** — Head and tail of a list
- **`append`** — Concatenate two lists
- **`reverse`** — Reverse a list
- **`empty?`** — Check if a list is empty
- **Mutual recursion** — Functions can call each other

## Why This Algorithm?

The N-Queens problem is a classic benchmark for:
1. **Backtracking** — Exploring a search space with pruning
2. **List operations** — Building and manipulating solutions
3. **Recursion** — Natural expression of the algorithm
4. **Predicate functions** — Checking constraints

This demo shows how Elle's list operations and recursive functions express the algorithm cleanly and idiomatically.

## Running the Demo

```bash
cargo run --release -- demos/nqueens/nqueens.lisp
```

To solve for a different N, edit the last line:
```janet
(benchmark 8)   # Solve for N=8 (92 solutions)
(benchmark 10)  # Solve for N=10 (724 solutions)
(benchmark 12)  # Solve for N=12 (14,200 solutions)
```

Larger values (N > 14) will take significantly longer due to exponential growth in the search space.
