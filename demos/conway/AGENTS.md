# conway

Conway's Game of Life rendered via SDL2 FFI.

## Responsibility

Demonstrate interactive graphics programming in Elle using SDL2 through the FFI system. Exercises library loading, struct marshalling, event polling, and mutable array manipulation.

Does NOT:
- Use any Elle GUI abstraction (raw FFI only)
- Require the async scheduler (synchronous main loop)

## Key files

| File | Purpose |
|------|---------|
| `conway.lisp` | Complete demo — SDL2 bindings, grid logic, event loop, rendering |

## FFI surface

- `libSDL2-2.0.so.0` — window, renderer, events, timing
- `target/release/libelle_random.so` — random float for grid randomization

## Running

```bash
cargo build --release -p elle-random
cargo run --release -- demos/conway/conway.lisp
```
