# CFG Visualizer Demo

Renders control flow graphs of Elle functions to SVG via DOT/graphviz.

## Prerequisites

- Elle (built with `cargo build --release`)
- [Graphviz](https://graphviz.org/) (`dot` command)

## Usage

```bash
make -C demos/cfgviz
```

This runs `cfgviz.lisp` to generate DOT files, then renders them to SVG.

## Functions visualized

| Function | Blocks | Demonstrates |
|----------|--------|-------------|
| `identity` | 1 | Single return block |
| `factorial` | 4 | Branch + recursive call |
| `fizzbuzz` | ~10 | Nested cond branches |
| `make-adder` | 1 | Closure creation with capture |
| `eval-expr` | ~30 | Match dispatch, recursion, error handling |

## Visual conventions

- **Colors**: blue = return, orange = branch, green = yield, grey = linear
- **Record shape**: header \| instructions \| terminator
- **Annotations**: `@line:col` shows source location for each instruction

## Cleaning up

```bash
make -C demos/cfgviz clean
```
