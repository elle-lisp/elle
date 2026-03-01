# elle-selkie

A Mermaid diagram rendering plugin for Elle, wrapping the Rust `selkie-rs` crate.

## Building

Built as part of the workspace:

```sh
cargo build --workspace
```

Produces `target/debug/libelle_selkie.so` (or `target/release/libelle_selkie.so`).

## Usage

```lisp
(import-file "path/to/libelle_selkie.so")

(def svg (selkie/render "flowchart LR; A-->B-->C"))
(print svg)  ;; => SVG string

(selkie/render-to-file "flowchart TD; X-->Y-->Z" "diagram.svg")

(def ascii (selkie/render-ascii "flowchart LR; A-->B"))
(print ascii)  ;; => ASCII art string
```

## Primitives

| Name | Args | Returns |
|------|------|---------|
| `selkie/render` | diagram | SVG string |
| `selkie/render-to-file` | diagram, path | path string |
| `selkie/render-ascii` | diagram | ASCII art string |
