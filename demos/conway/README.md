# Conway's Game of Life

## What This Demo Does

Interactive cellular automaton rendered via `std/sdl3` (pure SDL3 FFI).
An 80×60 grid of cells at 10px each gives an 800×600 window with vsync.

**Key features demonstrated:**
- Module import (`(import "std/sdl3")`, closure-as-module pattern)
- SDL3 window, renderer, events, timing, debug text, blend modes
- Event-driven main loop with keyboard, mouse click, and mouse drag
- Mutable flat array as a grid (row-major indexing)
- Dynamic window title updates
- Grid line overlay with alpha blending
- HUD with generation counter, alive count, FPS, and speed indicator
- Self-contained PRNG (no plugin dependency)

## Controls

| Key | Action |
|-----|--------|
| Click / drag | Toggle / paint cells |
| Space | Pause / resume |
| G | Toggle grid lines |
| R | Randomize grid |
| C | Clear grid |
| + / - | Speed up / slow down (steps per frame) |
| Escape / Q | Quit |

## Seed Patterns

The initial grid places several well-known patterns:
- **R-pentomino** — a methuselah that evolves for 1103 generations
- **Gliders** — diagonal travelers
- **LWSS** — lightweight spaceship
- **Pulsar** — period-3 oscillator with four-way symmetry

## Dependencies

- `libSDL3.so` — SDL3 runtime library

## Running

```bash
cargo run --release -- demos/conway/conway.lisp
```
