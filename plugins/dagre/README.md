# elle-dagre

Hierarchical graph layout plugin for Elle using the
[dagre-rs](https://crates.io/crates/dagre-rs) crate (Sugiyama method via
petgraph).

## Primitives

### `dagre/layout`

Compute layout positions for a directed graph.

**Args:** `nodes` `edges`

- `nodes` — list of `(id "label")` or `(id "label" width height)`
- `edges` — list of `(from to)` where `from`/`to` are node ids

**Returns:** struct `{:positions <list> :width <num> :height <num>}`

Each position is a struct `{:id <int> :x <float> :y <float>}`.

### `dagre/render`

Compute layout and render to an SVG string.

**Args:** same as `dagre/layout`

**Returns:** SVG string

## Interface compatibility

Both primitives use the same input format as the `sugiyama` plugin, so
the two are interchangeable.
