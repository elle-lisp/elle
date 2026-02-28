# elle-sugiyama

Hierarchical graph layout plugin for Elle, wrapping the
[rust-sugiyama](https://crates.io/crates/rust-sugiyama) crate (Sugiyama's
algorithm for layered directed graph drawing).

## Primitives

### `sugiyama/layout`

Compute layout positions for a directed graph.

**Args:**
- `nodes` — list of `(id "label")` or `(id "label" width height)`
- `edges` — list of `(from to)` pairs

**Returns:** struct `{:positions <list> :width <num> :height <num>}`

Each position in the list is a struct `{:id <int> :x <float> :y <float>}`.

```lisp
(import "plugins/sugiyama/target/debug/libelle_sugiyama.so")

(def result (sugiyama/layout
  '((0 "Start") (1 "Middle") (2 "End"))
  '((0 1) (1 2))))

(:positions result)  # => list of {:id :x :y} structs
(:width result)      # => total layout width
(:height result)     # => total layout height
```

### `sugiyama/render`

Compute layout and render to an SVG string.

**Args:** same as `sugiyama/layout`

**Returns:** SVG string

```lisp
(def svg (sugiyama/render
  '((0 "Reader") (1 "Expander") (2 "Analyzer") (3 "Lowerer") (4 "Emitter"))
  '((0 1) (1 2) (2 3) (3 4))))

(spit "pipeline.svg" svg)
```

## Node format

Nodes are lists of 2-4 elements:

| Element | Type | Required | Default |
|---------|------|----------|---------|
| id | integer | yes | — |
| label | string | yes | — |
| width | number | no | `len(label) * 8 + 20` |
| height | number | no | `30` |

## Edge format

Edges are lists of exactly 2 integer elements: `(from-id to-id)`.
