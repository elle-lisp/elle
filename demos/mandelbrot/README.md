# Mandelbrot Explorer

## What This Demo Does

Interactive Mandelbrot set viewer rendered via GTK4 and Cairo FFI.
Computes the fractal in Elle, writes to an ARGB32 pixel buffer, and
blits it to a GTK4 drawing area through a temporary Cairo image surface.

Uses `lib/gtk4/bind.lisp` for shared GTK4/GLib/GObject bindings,
adding only the Cairo and drawing-area bindings specific to this demo.

**Key features demonstrated:**
- Reusing `std/gtk4/bind` for GTK4 library handles and common bindings
- GTK4 application lifecycle via FFI (`GtkApplication`, `g_application_run`)
- FFI callbacks (`ffi/callback`) for GTK signals and draw functions
- Cairo image surface creation from raw pixel data
- GTK4 event controllers (gesture click, scroll, keyboard)
- Row-at-a-time pixel buffer writes using `ffi/array` type

## Controls

| Key | Action |
|-----|--------|
| Left click | Zoom in (2x) centered on cursor |
| Right click | Zoom out (2x) centered on cursor |
| Scroll | Zoom in / out at center |
| Arrow keys | Pan |
| + / = | Double max iterations |
| - | Halve max iterations |
| R | Reset view |
| Escape / Q | Quit |

## How It Works

### Rendering pipeline

1. `compute-mandelbrot` iterates each pixel, maps it to the complex
   plane, and runs the escape-time iteration `z = z² + c`
2. Escaped pixels are colored via a precomputed Bernstein polynomial
   palette with smooth coloring (log2 renormalization)
3. Each row is written to a `ffi/malloc`'d ARGB32 buffer using
   `ffi/write` with an `ffi/array :u32 WIDTH` type
4. The GTK draw callback creates a temporary Cairo image surface from
   the buffer, scales it, and paints it

### Color palette

Bernstein polynomial palette (256 colors):
- `r = 9(1-t)t³` — blue at low t, red at high t
- `g = 15(1-t)²t²` — green in the middle
- `b = 8.5(1-t)³t` — blue at low t

### Smooth coloring

Log2 renormalization gives fractional iteration counts:
```
smooth = iter + 1 - log₂(log(|z|))
```

## Dependencies

- `libgtk-4.so.1` — GTK4 (via `std/gtk4/bind`)
- `libgobject-2.0.so` — GObject signals (via `std/gtk4/bind`)
- `libgio-2.0.so.0` — GApplication
- `libcairo.so.2` — Cairo image surface and painting

## Running

```bash
elle demos/mandelbrot/mandelbrot.lisp
```
