# conway

Conway's Game of Life rendered via `std/sdl` (SDL3 pure FFI).

## Responsibility

Demonstrate interactive graphics programming in Elle using SDL3 through
the `std/sdl` library. Exercises module import, event handling
(keyboard + mouse), draw primitives, debug text, blend modes, and
dynamic window titles.

Does NOT:
- Use raw FFI directly (uses std/sdl)
- Require any Rust plugins
- Require the async scheduler (synchronous main loop)

## Key files

| File | Purpose |
|------|---------|
| `conway.lisp` | Complete demo — grid logic, event loop, rendering |

## Dependencies

- `std/sdl` via `(import "std/sdl")`
- `libSDL3.so` system library

## Running

```bash
cargo run --release -- demos/conway/conway.lisp
```
