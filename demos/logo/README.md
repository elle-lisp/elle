# Logo generator

<!-- audited: 2026-10-28 -->

Draws the Elle glyph as bundles of colored fibers along cubic Bézier curves, and writes it as an SVG file.

## What it draws

The glyph is a leaning Y of three strokes. A diagonal runs from the top right
down to a junction, a base runs from the junction to the right, and an arm
leaves the diagonal and runs up to the left. Each stroke is one cubic Bézier
curve, and the base is a straight line promoted to one.

Each stroke is seven parallel fibers, offset from the curve along its normal,
in a gradient from yellow through red to black. The offset tapers along the
stroke, from the full width in the middle to three tenths of it at each end.

## How it computes

[logo.lisp](logo.lisp) is the whole program:

| Part | Functions |
|------|-----------|
| 2D vectors, as two-element arrays | `v+`, `v-`, `v*`, `vlength` |
| A curve's point, tangent and unit normal at `t` | `bezier-eval`, `bezier-tangent`, `normal-at` |
| A fiber's offset points, at 41 even steps of `t` on each curve | `offset-point`, `taper`, `fiber-path-data` |
| The SVG document, one `<path>` for each fiber | `svg-path`, `render-fibers`, `build-svg` |

## Running

Run the demo from the repository root. It writes
[docs/logo.svg](../../docs/logo.svg), a path relative to the directory it runs
in.

```bash
elle demos/logo/logo.lisp
```
