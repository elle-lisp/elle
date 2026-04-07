# conway

Conway's Game of Life rendered via `std/sdl3` (SDL3 pure FFI).

## Responsibility

Demonstrate interactive graphics programming in Elle using SDL3 through
the `std/sdl3` library. Exercises module import, event handling
(keyboard + mouse), draw primitives, debug text, blend modes, and
dynamic window titles.

Does NOT:
- Use raw FFI directly (uses std/sdl3)
- Require any Rust plugins
- Require the async scheduler (synchronous main loop)

## Key files

| File | Purpose |
|------|---------|
| `conway.lisp` | Complete demo — grid logic, event loop, rendering |

## Dependencies

- `std/sdl3` via `(import "std/sdl3")`
- `libSDL3.so` system library

## Running

```bash
cargo run --release -- demos/conway/conway.lisp
```
