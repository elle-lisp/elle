(elle/epoch 8)

# CFG Visualizer Demo
#
# Renders control flow graphs of Elle functions to DOT format.
# Use the Makefile to convert DOT files to SVG via graphviz.
#
# Usage:
#   make -C demos/cfgviz

# ── Functions to visualize ───────────────────────────────────────────

(defn identity [x]
  "The simplest function — one block, one return."
  x)

(defn factorial [n]
  "Recursive factorial — branching and self-call."
  (if (< n 2)
    1
    (* n (factorial (- n 1)))))

(defn fizzbuzz [n]
  "Classic fizzbuzz — nested branching."
  (cond
    ((= (mod n 15) 0) "fizzbuzz")
    ((= (mod n 3) 0)  "fizz")
    ((= (mod n 5) 0)  "buzz")
    (true              n)))

(defn make-adder [x]
  "Returns a closure — shows captured variable in LIR."
  (fn [y] (+ x y)))

(defn eval-expr [expr]
  "Evaluate an arithmetic expression tree.
   Match dispatch, recursion, let-binding, conditional error —
   produces a complex CFG with many blocks and cross-edges."
  (match expr
    ([:lit n]   n)
    ([:neg a]   (- 0 (eval-expr a)))
    ([:add a b] (+ (eval-expr a) (eval-expr b)))
    ([:sub a b] (- (eval-expr a) (eval-expr b)))
    ([:mul a b] (* (eval-expr a) (eval-expr b)))
    ([:div a b]
      (let* [divisor (eval-expr b)
             dividend (eval-expr a)]
        (if (= divisor 0)
          (error {:error :division-by-zero :message "division by zero in expression"})
          (/ dividend divisor))))
    (_ (error "unknown expression"))))

# ── Render each function to DOT ─────────────────────────────────────

(defn render-cfg [f name]
  "Render a function's CFG to a DOT file."
  (let* [dot (fn/cfg f :dot)
         path (string "demos/cfgviz/" name ".dot")]
    (file/write path dot)
    (println "  wrote " path)))

(println "Rendering control flow graphs to DOT...")
(render-cfg identity "identity")
(render-cfg factorial "factorial")
(render-cfg fizzbuzz "fizzbuzz")
(render-cfg make-adder "make-adder")
(render-cfg eval-expr "eval-expr")
(println "Done. Run 'make -C demos/cfgviz' to generate SVGs.")
