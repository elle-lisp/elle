# mandelbrot

Interactive Mandelbrot set explorer using GTK4 + Cairo via FFI.

## Architecture

Uses `std/gtk4/bind` for shared GTK4/GLib/GObject library handles and
common widget bindings. Adds Cairo and GtkDrawingArea bindings locally
since these are rendering-specific and not part of the widget toolkit
module.

## Key files

| File | Purpose |
|------|---------|
| `mandelbrot.lisp` | Complete demo — fractal computation, Cairo rendering, event handling |

## Dependencies

- `std/gtk4/bind` — GTK4 library handles and common bindings
- `libgio-2.0.so.0` — GApplication lifecycle
- `libcairo.so.2` — pixel buffer rendering

## Running

```bash
elle demos/mandelbrot/mandelbrot.lisp
```
