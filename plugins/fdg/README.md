# elle-fdg

Force-directed graph layout plugin for Elle using [fdg-sim](https://crates.io/crates/fdg-sim) (MIT licensed).

## Layout algorithm

Uses the Fruchterman-Reingold force-directed algorithm. Nodes repel each other
while edges act as springs pulling connected nodes together. The result is an
organic, spring-like layout — contrast with hierarchical layout algorithms like
Sugiyama or Dagre which arrange nodes in ranked layers.

## Primitives

### `fdg/layout` (arity 2)

Compute layout positions without rendering.

**Args:**
- `nodes` — list of `(id "label")` pairs
- `edges` — list of `(from to)` pairs (node ids)

**Returns:** struct `{:positions <list> :width <num> :height <num>}`

Each position is a struct `{:id <int> :label <string> :x <float> :y <float>}`.

```lisp
(def layout (fdg/layout '((0 "A") (1 "B") (2 "C"))
                        '((0 1) (1 2))))
```

### `fdg/render` (arity 2)

Compute layout and render to an SVG string.

**Args:** same as `fdg/layout`

**Returns:** SVG string

```lisp
(def svg (fdg/render '((0 "A") (1 "B") (2 "C"))
                     '((0 1) (1 2))))
```

## License

fdg-sim is MIT licensed. This plugin intentionally avoids fdg-img (GPL-3.0)
and renders SVG directly.
