# Heat Diffusion Simulation

## What This Demo Does

This demo simulates heat spreading across a 2D grid using the discrete Laplacian equation. It's a classic numerical simulation that demonstrates:
- Matrix creation and indexing
- Element-wise operations with `map`
- Stencil computation (neighbor averaging)
- Functional iteration
- ASCII visualization with a temperature gradient

The simulation starts with a single hot cell in the center and shows how heat diffuses to neighboring cells over time.

## How It Works

### Matrix Representation

Matrices are represented as tuples of tuples (immutable, row-major):
```janet
(defn make-matrix [rows cols initial-value]
  "Create an m×n matrix (tuple of tuples, row-major)."
  (tuple ;(map (fn [_] (tuple ;(map (fn [_] initial-value) (range cols))))
               (range rows))))
```

A 3×3 matrix of zeros:
```
#[#[0 0 0]
  #[0 0 0]
  #[0 0 0]]
```

### Matrix Operations

**`matrix-ref`** — Get element at (i, j)
```janet
(defn matrix-ref [m i j]
  (get (get m i) j))
```

**`matrix-set`** — Return a new matrix with element at (i, j) changed
```janet
(defn matrix-set [m i j val]
  (let* ([row (get m i)]
         [new-row (tuple ;(map (fn [k v] (if (= k j) val v)) (range (length row)) row))]
         [rows (map (fn [k r] (if (= k i) new-row r)) (range (length m)) m)])
    (tuple ;rows)))
```

This uses `map` with index tracking to rebuild the matrix with one element changed.

**`matrix-map`** — Apply a function to every element
```janet
(defn matrix-map [f m]
  (tuple ;(map (fn [row]
                 (tuple ;(map f row)))
               m)))
```

**`matrix-add`** — Element-wise addition
```janet
(defn matrix-add [m1 m2]
  (tuple ;(map (fn [r1 r2]
                 (tuple ;(map + r1 r2)))
               m1 m2)))
```

### The Diffusion Algorithm

Heat diffusion is governed by the discrete Laplacian:
```
new[i][j] = cell * (1 - 4*alpha) + alpha * (up + down + left + right)
```

Where:
- `cell` is the current temperature
- `up`, `down`, `left`, `right` are the temperatures of neighboring cells
- `alpha` is the diffusion coefficient (controls how fast heat spreads)
- Boundary cells are fixed at 0 (cold walls)

```janet
(defn diffuse-step [m alpha]
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
```

### Visualization

Temperature is mapped to a 10-character gradient:
```janet
(defn temperature-to-char [temp]
  (let* ([scaled (math/floor (* temp 9.0))]
         [clamped (if (> scaled 9) 9 scaled)]
         [chars " .:-=+*#%@"]
         [idx (if (< clamped 0) 0 clamped)])
    (string/char-at chars idx)))
```

- ` ` (space) = cold (0.0)
- `.` = slightly warm
- `@` = hot (1.0)

## Sample Output

The demo runs a 16×16 grid for 12 time steps with α=0.2:

```
=== Heat Diffusion Simulation (Pure Elle) ===
Temperature gradient: ' ' (cold) → '@' (hot)

                
                
                
                
                
                
                
                
        @       
                
                
                
                
                
                
                

                
                
                
                
                
                
                
        @       
                
                
                
                
                
                
                

                
                
                
                
                
                
       :-       
       @:       
                
                
                
                
                
                
                

...
=== Complete ===
```

Each frame shows the grid at a different time step. The `@` (hot spot) gradually spreads outward as heat diffuses to neighbors.

## Elle Idioms Used

- **`defn`** — Function definition
- **`let*`** — Sequential bindings
- **`map`** — Transform every element of a sequence
- **`fold`** — Reduce a sequence to a single value
- **`range`** — Generate a sequence of integers
- **`tuple`** — Immutable sequence (used for matrices)
- **`each`** — Iterate over a sequence (used in rendering)
- **Tuple unpacking with `;`** — Splice operator to flatten nested sequences

## Why This Algorithm?

Heat diffusion is a classic numerical simulation because:
1. **Physical intuition** — Everyone understands heat spreading
2. **Numerical stability** — The discrete Laplacian is well-studied
3. **Visualization** — ASCII output is easy to understand
4. **Performance** — Stencil computations are common in scientific computing

This demo shows how Elle's functional operations (`map`, `fold`) and immutable data structures express numerical algorithms cleanly.

## Running the Demo

```bash
cargo run --release -- demos/matrix.lisp
```

To modify the simulation, edit the parameters at the bottom:
```janet
(let* ([rows 16]      # Grid height
       [cols 16]      # Grid width
       [alpha 0.2]    # Diffusion coefficient (0 < alpha < 0.25)
       [steps 12]     # Number of time steps
       [grid (initialize-grid rows cols)])
  (simulate grid steps alpha))
```

Larger grids and more steps will take longer. The diffusion coefficient `alpha` controls how fast heat spreads (larger = faster).
