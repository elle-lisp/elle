# Progress Bar OSD

Wayland overlay progress bar. Reads percentages (0–100) from stdin, one per
line, and renders a pill-shaped bar with smooth ease-out transitions.

## Usage

```bash
# Single value
echo "50" | cargo run --release -- demos/progress-bar/progress-bar.lisp

# Animated sequence
seq 0 5 100 | cargo run --release -- demos/progress-bar/progress-bar.lisp

# Pipe from a slow process
(some-long-process) | cargo run --release -- demos/progress-bar/progress-bar.lisp

# fish shell
for i in (seq 0 5 100); echo $i; sleep 0.15; end | cargo run --release -- demos/progress-bar/progress-bar.lisp
```

## Dependencies

- `plugin/wayland` — Wayland compositor interaction
- `std/wayland` — Elle wrapper for the wayland plugin
- A Wayland compositor with `wlr-layer-shell` support (Sway, Hyprland, River, etc.)

## What it does

1. Connects to the Wayland compositor
2. Creates a full-screen transparent overlay at the top layer
3. Waits for the compositor to configure the surface
4. Creates an ARGB8888 SHM buffer
5. Spawns a fiber to read stdin percentages
6. Animates the bar toward each new value with ease-out interpolation
7. Cleans up the overlay on EOF

The bar is 50% of screen width, centered horizontally, positioned 25% up
from the bottom edge, with height at 3% of screen height.
