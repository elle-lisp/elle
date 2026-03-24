# Mandelbrot Explorer

## What This Demo Does

Interactive Mandelbrot set viewer rendered via GTK4 and Cairo FFI. Computes the fractal in Elle, writes to an ARGB32 pixel buffer, and blits it to a GTK4 drawing area through a temporary Cairo image surface.

**Key features demonstrated:**
- GTK4 application lifecycle via FFI (`GtkApplication`, `g_signal_connect_data`, `g_application_run`)
- FFI callbacks (`ffi/callback`) for GTK signals and draw functions
- Cairo image surface creation from raw pixel data
- GObject signal system (`g_signal_connect_data` with Elle closures as C function pointers)
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

1. `compute-mandelbrot` iterates each pixel, maps it to the complex plane, and runs the escape-time iteration `z = z^2 + c`
2. Escaped pixels are colored via a precomputed Bernstein polynomial palette with smooth coloring (log2 renormalization)
3. Each row is written to a `ffi/malloc`'d ARGB32 buffer using `ffi/write` with an `ffi/array :u32 WIDTH` type
4. The GTK draw callback creates a temporary `cairo_image_surface_create_for_data` from the buffer, scales it via `cairo_scale`, and paints it

### Color palette

Bernstein polynomial palette (256 colors) gives the classic Mandelbrot look:
- `r = 9(1-t)t^3` — blue at low t, red at high t
- `g = 15(1-t)^2 t^2` — green in the middle
- `b = 8.5(1-t)^3 t` — blue at low t

### Smooth coloring

Instead of banding by integer iteration count, the demo uses log2 renormalization:

```
smooth = iter + 1 - log2(log(|z|))
```

This gives fractional iteration counts that produce smooth color gradients.

### Performance

At 800x600 with 32 max iterations, the initial full-set render takes ~3s. Zoomed-in views with fewer set-interior pixels render faster. Increasing max iterations (for deep zoom detail) linearly increases render time for interior pixels.

## Dependencies

- `libgtk-4.so.1` — GTK4
- `libgobject-2.0.so.0` — GObject signal system
- `libgio-2.0.so.0` — GApplication
- `libcairo.so.2` — Cairo image surface and painting

## Running

```bash
cargo run --release -- demos/mandelbrot/mandelbrot.lisp
```

## Known issues

- `(block (def a @[]) (push a x) a)` returns a broken heap reference — the demo uses `let*` instead as a workaround
- At very deep zoom (scale < ~1e-14), double-precision floating point exhausts its mantissa and the fractal stops resolving. Increase max iterations with + to see detail at moderate zoom depths.
