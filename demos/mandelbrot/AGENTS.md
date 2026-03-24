# mandelbrot

Interactive Mandelbrot set explorer rendered via GTK4 + Cairo FFI.

## Responsibility

Demonstrate GTK4 widget-based GUI programming in Elle using FFI callbacks. Exercises the full GObject signal system, Cairo pixel-buffer rendering, and GTK4 event controllers.

Does NOT:
- Use any Elle GUI abstraction (raw FFI only)
- Require the async scheduler (synchronous computation, GTK main loop)

## Key files

| File | Purpose |
|------|---------|
| `mandelbrot.lisp` | Complete demo — GTK4/Cairo bindings, Mandelbrot computation, event handling |

## FFI surface

- `libgtk-4.so.1` — application, window, drawing area, event controllers
- `libgobject-2.0.so.0` — `g_signal_connect_data` for signal wiring
- `libgio-2.0.so.0` — `g_application_run` for main loop
- `libcairo.so.2` — image surface creation, scaling, painting
- libc (`nil` handle) — `clock_gettime` for render timing

## Architecture

```
GtkApplication
  └─ activate signal (ffi/callback)
       └─ GtkApplicationWindow
            └─ GtkDrawingArea
                 ├─ draw_func (ffi/callback) — blit ARGB32 buffer via Cairo
                 ├─ GtkGestureClick — zoom in/out on click
                 └─ GtkEventControllerScroll — zoom on scroll
            └─ GtkEventControllerKey — pan, reset, quit, iteration control
```

## Running

```bash
cargo run --release -- demos/mandelbrot/mandelbrot.lisp
```
