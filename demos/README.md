# Elle Demos

Demonstration programs that dogfood Elle with non-trivial algorithms and serve as cross-language comparison implementations.

## Demos

| Demo | Purpose |
|------|---------|
| [blas/](blas/) | BLAS/LAPACK FFI — optimized linear algebra via CBLAS and LAPACKE |
| [cfgviz/](cfgviz/) | Control flow graph visualization to DOT/SVG |
| [conway/](conway/) | Conway's Game of Life — interactive SDL2 demo |
| [docgen/](docgen/) | Documentation site generator written in Elle |
| [egui/](egui/) | Immediate-mode GUI via egui plugin |
| [fib/](fib/) | Recursive Fibonacci benchmark measuring function call overhead |
| [logo/](logo/) | Elle logo glyph rendered as colored bezier fibers |
| [mandelbrot/](mandelbrot/) | Interactive Mandelbrot explorer — GTK4 + Cairo (uses `std/gtk4/bind`) |
| [matrix/](matrix/) | Heat diffusion simulation with ASCII visualization |
| [microgpt/](microgpt/) | Micro GPT — scalar autograd + character-level transformer |
| [nqueens/](nqueens/) | N-Queens backtracking algorithm (cons-list and array variants) |
| [scope-alloc/](scope-alloc/) | Scope allocation workload measuring escape analysis tiers |
| [webserver/](webserver/) | HTTP server + concurrent load generator with latency stats |

## Running Demos

```bash
elle demos/fib/fib.lisp
elle demos/nqueens/nqueens.lisp
elle demos/nqueens/nqueens-array.lisp
elle demos/blas/blas.lisp
elle demos/matrix/matrix.lisp
elle demos/cfgviz/cfgviz.lisp
elle demos/scope-alloc/scope-alloc.lisp
elle demos/logo/logo.lisp
elle demos/docgen/generate.lisp
elle demos/egui/smoke.lisp

# Networking (requires two terminals)
elle demos/webserver/server.lisp          # terminal 1
elle demos/webserver/loadgen.lisp         # terminal 2

# Interactive (require display + libraries)
elle demos/conway/conway.lisp         # SDL2
elle demos/mandelbrot/mandelbrot.lisp # GTK4 + Cairo
elle demos/egui/counter.lisp          # egui plugin
```
