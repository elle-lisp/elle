# Control Flow Graph Visualization Demo

## What This Demo Does

This demo generates control flow graphs (CFGs) of Elle functions in DOT format, which can be visualized using Graphviz. It demonstrates:
- Introspection of compiled functions
- Control flow analysis
- Graph visualization
- File I/O

The demo includes five example functions with varying complexity:
1. **identity** — Simplest: one block, one return
2. **factorial** — Recursion and branching
3. **fizzbuzz** — Nested conditionals
4. **make-adder** — Closures and captured variables
5. **eval-expr** — Complex: match dispatch, recursion, error handling

## How It Works

### What is a Control Flow Graph?

A control flow graph (CFG) is a directed graph where:
- **Nodes** represent basic blocks (sequences of instructions with no branches)
- **Edges** represent control flow (jumps, branches, function calls)

CFGs are used for:
- **Compiler optimization** — Identifying dead code, loop invariants, etc.
- **Program analysis** — Understanding execution paths
- **Debugging** — Tracing how code executes
- **Documentation** — Showing the structure of complex functions

### Generating CFGs

Elle provides the `fn/cfg` primitive to extract a function's CFG:

```janet
(defn render-cfg (f name)
  "Render a function's CFG to a DOT file."
  (let* ((dot (fn/cfg f :dot))
         (path (append "demos/cfgviz/" (append name ".dot"))))
    (file/write path dot)
    (display (append "  wrote " (append path "\n")))))
```

The `:dot` format is the DOT language used by Graphviz.

## Functions Visualized

| Function | Blocks | Demonstrates |
|----------|--------|-------------|
| `identity` | 1 | Single return block |
| `factorial` | 4 | Branch + recursive call |
| `fizzbuzz` | ~10 | Nested cond branches |
| `make-adder` | 1 | Closure creation with capture |
| `eval-expr` | ~30 | Match dispatch, recursion, error handling |

### Example: Factorial

```janet
(defn factorial (n)
  "Recursive factorial — branching and self-call."
  (if (< n 2)
    1
    (* n (factorial (- n 1)))))
```

The CFG shows:
- Entry block
- Branch on `(< n 2)`
- True branch: return 1
- False branch: recursive call and multiplication
- Return block

### Example: Eval-Expr

```janet
(defn eval-expr (expr)
  "Evaluate an arithmetic expression tree."
  (match expr
    ([:lit n]   n)
    ([:neg a]   (- 0 (eval-expr a)))
    ([:add a b] (+ (eval-expr a) (eval-expr b)))
    ([:sub a b] (- (eval-expr a) (eval-expr b)))
    ([:mul a b] (* (eval-expr a) (eval-expr b)))
    ([:div a b]
      (let* ([divisor (eval-expr b)]
             [dividend (eval-expr a)])
        (if (= divisor 0)
          (error [:division-by-zero "division by zero in expression"])
          (/ dividend divisor))))
    (_ (error "unknown expression"))))
```

This produces a complex CFG with:
- Multiple match arms (one block per pattern)
- Recursive calls
- Let bindings
- Error handling
- Type guards

## Visual Conventions

- **Colors**: blue = return, orange = branch, green = yield, grey = linear
- **Record shape**: header | instructions | terminator
- **Annotations**: `@line:col` shows source location for each instruction

## Sample Output

The demo generates five DOT files:

```
Rendering control flow graphs to DOT...
  wrote demos/cfgviz/identity.dot
  wrote demos/cfgviz/factorial.dot
  wrote demos/cfgviz/fizzbuzz.dot
  wrote demos/cfgviz/make-adder.dot
  wrote demos/cfgviz/eval-expr.dot
Done. Run 'make -C demos/cfgviz' to generate SVGs.
```

## Elle Idioms Used

- **`defn`** — Function definition
- **`let*`** — Sequential bindings
- **`match`** — Pattern matching with multiple arms
- **`fn/cfg`** — Extract a function's control flow graph
- **`file/write`** — Write a string to a file
- **`append`** — String concatenation
- **`display`** — Print to stdout

## Prerequisites

- Elle (built with `cargo build --release`)
- [Graphviz](https://graphviz.org/) (`dot` command)

## Usage

Generate DOT files:
```bash
cargo run --release -- demos/cfgviz/cfgviz.lisp
```

Convert to SVG:
```bash
make -C demos/cfgviz
```

Or do both at once:
```bash
make -C demos/cfgviz all
```

View the SVGs:
```bash
open demos/cfgviz/identity.svg
open demos/cfgviz/factorial.svg
# ... etc
```

## Cleaning up

```bash
make -C demos/cfgviz clean
```

## Why This Demo?

Control flow graphs are useful for:
1. **Understanding code structure** — Visualizing how functions execute
2. **Compiler development** — Analyzing optimization opportunities
3. **Debugging** — Tracing execution paths
4. **Documentation** — Showing the structure of complex functions
5. **Teaching** — Explaining how compilers work

This demo shows that Elle can introspect its own compiled code and generate useful visualizations.

## Further Reading

- [Control Flow Graph (Wikipedia)](https://en.wikipedia.org/wiki/Control-flow_graph)
- [DOT Language](https://graphviz.org/doc/info/lang.html)
- [Graphviz Documentation](https://graphviz.org/)
- [Compiler Design — Control Flow Analysis](https://en.wikipedia.org/wiki/Data-flow_analysis)
