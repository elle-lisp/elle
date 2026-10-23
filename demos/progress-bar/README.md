# Progress Bar OSD

<!-- audited: 2026-10-23 -->

A Wayland overlay that draws a progress bar and eases it toward each
percentage, 0 to 100, read from stdin one per line.

[progress-bar.lisp](progress-bar.lisp) is the whole demo. It needs
`plugin/wayland`, which `std/wayland` wraps, and a compositor with
`wlr-layer-shell` support, such as Sway, Hyprland or River.

## Running

```bash
# One value
echo "50" | elle demos/progress-bar/progress-bar.lisp

# A sequence
seq 0 5 100 | elle demos/progress-bar/progress-bar.lisp

# fish shell
for i in (seq 0 5 100); echo $i; sleep 0.15; end | elle demos/progress-bar/progress-bar.lisp
```

## Two fibers

| Fiber | What it does |
|-------|--------------|
| The stdin reader | Reads a line with `port/read-line`, parses it with `parse-int`, and sets `@target-pct`. At the end of stdin it sets `@done`. |
| The main loop | Moves `@display-pct` 15% of the way to `@target-pct` each frame, and snaps to it within half a point. It then renders the bar and pumps the Wayland events. |

The main loop runs until the reader sets `@done`, then destroys the overlay
and disconnects.

## The event loop

Each frame runs `wl:flush`, `ev/poll-fd` on the connection's descriptor with
a 33 ms timeout, `wl:dispatch`, then `wl:poll-events`. `wl:dispatch` reads
the wire without blocking and then dispatches what arrived, so the poll comes
first: `ev/poll-fd` yields to the scheduler until the descriptor is readable,
and the stdin reader runs in the meantime. A loop of the same shape waits for
the first `:configure` event, which gives the screen's size.

## The bar

The overlay is a layer surface on the `:overlay` layer, anchored to all four
edges, so it covers the screen. One ARGB8888 shared-memory buffer covers it
too.

The bar is half the screen wide, centered, with its middle a quarter of the
screen up from the bottom edge. Its height is 3% of the screen's. Each frame
clears the buffer and draws a pill: two circles from `wl:buffer-fill-circle`
for the end caps, and a rectangle from `wl:buffer-fill-rect` between them. The
fill is a narrower pill over the track, and a fill narrower than the bar's
height is one circle.

| Element | Color | Description |
|---------|-------|-------------|
| Track | `0x55000000` | Transparent black |
| Fill | `0xDD7EC8E3` | Light blue, about 87% opaque |
| Background | `0x00000000` | Fully transparent |
