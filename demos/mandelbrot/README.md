# Mandelbrot Explorer

<!-- audited: 2026-10-23 -->

An interactive Mandelbrot set viewer in a GTK4 window, computed on the GPU when
Vulkan is present and on a pool of threads when it is not.

[mandelbrot.lisp](mandelbrot.lisp) is the whole demo. It writes each frame to
an ARGB32 pixel buffer from `ffi/malloc`, and the draw callback paints that
buffer through a temporary Cairo image surface.

## Libraries

| Import | What the demo takes from it |
|--------|-----------------------------|
| `std/gtk4` | The application, the window, the drawing area and its event handlers |
| `std/gtk4/bind` | The window size, fullscreen, and the button a click gesture reports |
| `std/cairo` | The image surface over the pixel buffer, its scale and its paint |
| `plugin/vulkan`, `std/gpu` | The compute kernel, when both load and the GPU starts |

The import of `std/gtk4/bind` opens GTK4, GLib, GObject, GIO, WebKitGTK and
JavaScriptCore, so the demo needs all six installed. `std/cairo` opens Cairo.

## The two backends

| Backend | When | How it computes a frame |
|---------|------|-------------------------|
| GPU | `std/gpu` starts a Vulkan device | One compiled kernel, one invocation per pixel. The viewport and the iteration limit arrive in a parameter buffer, so the kernel compiles once. The window opens fullscreen at 1600×1200, at 256 iterations. |
| CPU | Otherwise | `NCPU` threads from `sys/spawn`, 16 unless the `NCPU` variable says otherwise. Each one takes a band of rows over a channel and writes each row with `ffi/write` as an `ffi/array :u32`. The window is 800×600, at 32 iterations. |

The CPU path skips the main cardioid and the period-2 bulb, and colors an
escaped pixel by its smooth iteration count, `iter + 1 - log₂(log |z|)`. Both
backends map an iteration count to a 256-entry Bernstein palette:

- `r = 9(1-t)t³`
- `g = 15(1-t)²t²`
- `b = 8.5(1-t)³t`

## Controls

| Key | Action |
|-----|--------|
| Left click | Zoom in (2x) centered on the cursor |
| Right click | Zoom out (2x) centered on the cursor |
| Scroll | Zoom in or out by 1.5x |
| Arrow keys | Pan |
| + / = | Double the iteration limit |
| - | Halve the iteration limit, down to 16 |
| R | Reset the view, at 32 iterations |
| Escape / Q | Quit |

## Running

```bash
elle demos/mandelbrot/mandelbrot.lisp
```
