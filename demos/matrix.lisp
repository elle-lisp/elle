# Heat Diffusion Simulation — Matrix Operations Demo
#
# Simulates heat spreading across a 2D grid using the discrete Laplacian.
# Each frame, heat diffuses from hot cells to their neighbors.
# Rendered as ASCII art with a temperature gradient.
#
# Demonstrates:
#   - Matrix creation and indexing
#   - Element-wise operations (map)
#   - Stencil computation (neighbor averaging)
#   - Functional iteration
#   - ASCII visualization

# ============================================================================
# Matrix Utilities
# ============================================================================

(defn make-matrix [rows cols initial-value]
  "Create an m×n matrix (tuple of tuples, row-major)."
  (tuple ;(map (fn [_] (tuple ;(map (fn [_] initial-value) (range cols))))
               (range rows))))

(defn matrix-ref [m i j]
  "Get element at (i, j)."
  (get (get m i) j))

(defn matrix-set [m i j val]
  "Return a new matrix with element at (i, j) set to val."
  (let* ([row (get m i)]
         [new-row (tuple ;(map (fn [k v] (if (= k j) val v)) (range (length row)) row))]
         [rows (map (fn [k r] (if (= k i) new-row r)) (range (length m)) m)])
    (tuple ;rows)))

(defn matrix-rows [m]
  "Get number of rows."
  (length m))

(defn matrix-cols [m]
  "Get number of columns."
  (if (> (length m) 0)
    (length (get m 0))
    0))

(defn matrix-map [f m]
  "Apply function f to every element, returning a new matrix."
  (tuple ;(map (fn [row]
                 (tuple ;(map f row)))
               m)))

(defn matrix-add [m1 m2]
  "Element-wise addition of two matrices."
  (tuple ;(map (fn [r1 r2]
                 (tuple ;(map + r1 r2)))
               m1 m2)))

# ============================================================================
# Heat Diffusion
# ============================================================================

(defn get-neighbor [m i j rows cols default]
  "Get element at (i, j), or default if out of bounds."
  (if (and (>= i 0) (< i rows) (>= j 0) (< j cols))
    (matrix-ref m i j)
    default))

(defn diffuse-step [m alpha]
  "Perform one diffusion step using the discrete Laplacian.
   new[i][j] = cell * (1 - 4*alpha) + alpha * (up + down + left + right)
   Boundary cells (edges) are fixed at 0 (cold walls)."
  (let* ([rows (matrix-rows m)]
         [cols (matrix-cols m)]
         [coeff (- 1.0 (* 4.0 alpha))])
    (tuple ;(map (fn [i]
                   (tuple ;(map (fn [j]
                                  (let* ([cell (matrix-ref m i j)]
                                         [up (get-neighbor m (- i 1) j rows cols 0.0)]
                                         [down (get-neighbor m (+ i 1) j rows cols 0.0)]
                                         [left (get-neighbor m i (- j 1) rows cols 0.0)]
                                         [right (get-neighbor m i (+ j 1) rows cols 0.0)]
                                         [neighbors (+ up down left right)])
                                    (+ (* cell coeff) (* alpha neighbors))))
                                (range cols))))
                 (range rows)))))

# ============================================================================
# Visualization
# ============================================================================

(defn temperature-to-char [temp]
  "Map temperature [0.0, 1.0] to a character gradient."
  (let* ([scaled (math/floor (* temp 9.0))]
         [clamped (if (> scaled 9) 9 scaled)]
         [chars " .:-=+*#%@"]
         [idx (if (< clamped 0) 0 clamped)])
    (string/char-at chars idx)))

(defn find-max [m]
  "Find the maximum value in the matrix."
  (fold (fn [acc row]
          (fold max acc row))
        0.0
        m))

(defn render-frame [m]
  "Render a matrix as ASCII art."
  (let* ([max-temp (find-max m)]
         [scale (if (> max-temp 0.0) (/ 1.0 max-temp) 1.0)])
    (each row in m
      (each cell in row
        (display (temperature-to-char (* cell scale))))
      (newline))))

# ============================================================================
# Simulation
# ============================================================================

(defn initialize-grid [rows cols]
  "Create a grid with a hot spot in the center."
  (let* ([center-i (/ rows 2)]
         [center-j (/ cols 2)])
    (tuple ;(map (fn [i]
                   (tuple ;(map (fn [j]
                                  (if (and (= i center-i) (= j center-j))
                                    1.0
                                    0.0))
                                (range cols))))
                 (range rows)))))

(defn simulate [grid steps alpha]
  "Run the diffusion simulation for a given number of steps."
  (if (<= steps 0)
    grid
    (begin
      (render-frame grid)
      (newline)
      (simulate (diffuse-step grid alpha) (- steps 1) alpha))))

# ============================================================================
# Main
# ============================================================================

(display "=== Heat Diffusion Simulation (Pure Elle) ===\n")
(display "Temperature gradient: ' ' (cold) → '@' (hot)\n")
(newline)

(let* ([rows 16]
       [cols 16]
       [alpha 0.2]
       [steps 12]
       [grid (initialize-grid rows cols)])
  (simulate grid steps alpha))

(display "=== Complete ===\n")
