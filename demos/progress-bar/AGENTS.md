# demos/progress-bar

OSD progress bar overlay for Wayland.

## Architecture

Two-fiber design:

- **Stdin fiber** — reads lines from stdin, parses as integers, updates `@target-pct`
- **Main loop** — eases `@display-pct` toward `@target-pct` at 15%/frame, renders bar, pumps wayland events

Rendering uses the wayland plugin's geometric primitives (fill-rect, fill-circle)
to draw a pill-shaped bar with rounded endcaps directly into an ARGB8888 SHM buffer.

## Data flow

```
stdin → port/read-line → parse-int → @target-pct
                                        ↓
main loop: ease display toward target → render-bar → buffer-fill-circle/rect → wl/commit
```

## Wayland plugin requirements

This demo uses primitives that were implemented alongside it:

- `wl/layer-surface` — accepts options struct (`:layer`, `:anchor`, `:width`, `:height`, `:exclusive-zone`)
- `wl/buffer-fill-rect` — fills a rectangular region
- `wl/buffer-fill-circle` — fills a circular region (for pill endcaps)
- `wl/dispatch` — does `prepare_read` + non-blocking `read` + `dispatch_pending`

The event loop pattern is: `flush → ev/poll-fd → dispatch → poll-events`.
This is required because `dispatch_pending` alone does not read from the wire;
`ev/poll-fd` yields to the scheduler, and `dispatch` reads + dispatches.

## Geometry

- Full-screen transparent overlay (all anchors)
- Bar: 50% screen width, centered, 25% from bottom, 3% screen height
- Pill shape: two semicircular endcaps + rectangle middle
- Fill animates from left with rounded leading edge

## Colors (ARGB8888)

| Element   | Color       | Description             |
|-----------|-------------|-------------------------|
| Track     | `0x55000000` | Transparent black       |
| Fill      | `0xDD7EC8E3` | Light blue, ~87% alpha  |
| Background| `0x00000000` | Fully transparent       |
