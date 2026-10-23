# Conway's Game of Life

<!-- audited: 2026-10-23 -->

An interactive cellular automaton in an SDL3 window, drawn through `std/sdl3`
with no plugin and no FFI of its own.

[conway.lisp](conway.lisp) is the whole demo. Its one import is `std/sdl3`,
which opens `libSDL3.so`, `libSDL3_image.so` and `libSDL3_ttf.so`.

## What it shows

- A closure as a module: `(import "std/sdl3")` answers the SDL3 bindings.
- A window, a renderer with vsync, debug text, lines, filled rectangles and an
  alpha-blended grid overlay.
- An event loop over keyboard, mouse clicks and mouse drags. The loop is
  synchronous: it polls SDL for events and never yields.
- A grid as one flat mutable array, row-major.
- A heads-up display of the generation, the live cells, the frame rate and the
  speed, with the generation in the window title.
- A xorshift generator of its own, so randomizing needs no plugin.

## The grid

The grid is 80×60 cells of 10 pixels, so the window opens at 800×600. Each
step reads the interior cells alone, and the border row and column stay dead:
the grid does not wrap.

The first grid places an R-pentomino, two gliders, a lightweight spaceship and
a pulsar, a period-3 oscillator. A randomized grid starts with about 28% of
its cells alive.

## Controls

| Key | Action |
|-----|--------|
| Click | Toggle a cell |
| Drag | Paint cells alive |
| Space | Pause / resume |
| G | Toggle grid lines |
| R | Randomize the grid |
| C | Clear the grid |
| + / - | One more or one fewer step per frame, from 1 to 20 |
| Escape / Q | Quit |

## Running

```bash
elle demos/conway/conway.lisp
```
