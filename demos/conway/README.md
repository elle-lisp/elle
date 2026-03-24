# Conway's Game of Life

## What This Demo Does

Interactive cellular automaton rendered via SDL2 FFI. An 80x60 grid of cells at 10px each gives an 800x600 window with vsync'd rendering.

**Key features demonstrated:**
- FFI library loading and function binding (`ffi/native`, `ffi/defbind`)
- FFI struct definitions and memory management (`ffi/struct`, `ffi/malloc`, `ffi/write`, `ffi/read`)
- Event-driven main loop (polling SDL events from a raw byte buffer)
- Mutable flat array as a grid (row-major indexing)
- Plugin import (`elle-random`)

## Controls

| Key | Action |
|-----|--------|
| Click | Toggle cell |
| Space | Pause / resume |
| R | Randomize grid |
| C | Clear grid |
| Escape / Q | Quit |

## How It Works

The grid is a flat mutable array of `ROWS * COLS` integers (0 or 1). Each generation, `step` creates a new grid by applying the standard Game of Life rules: a cell is alive if it has exactly 3 neighbors, or is already alive with exactly 2 neighbors.

Rendering writes each live cell as a filled rectangle via `SDL_RenderFillRect`, using a shared struct buffer to avoid per-cell allocation.

SDL events are read from a 64-byte `ffi/malloc`'d buffer with struct overlays for keyboard and mouse event types.

## Seed Patterns

The initial grid places several well-known patterns:
- **R-pentomino** — a methuselah that evolves for 1103 generations
- **Gliders** — diagonal travelers
- **LWSS** — lightweight spaceship
- **Pulsar** — period-3 oscillator with four-way symmetry

## Dependencies

- `libSDL2-2.0.so.0` — SDL2 runtime library
- `target/release/libelle_random.so` — Elle random plugin (build with `cargo build --release -p elle-random`)

## Running

```bash
cargo build --release -p elle-random
cargo run --release -- demos/conway/conway.lisp
```
