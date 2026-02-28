# elle-mermaid

A Mermaid diagram rendering plugin for Elle, wrapping the Rust `mermaid-rs-renderer` crate.

## Building

Built as part of the workspace:

```sh
cargo build --workspace
```

Produces `target/debug/libelle_mermaid.so` (or `target/release/libelle_mermaid.so`).

## Usage

```lisp
(import-file "path/to/libelle_mermaid.so")

(def svg (mermaid/render "flowchart LR; A-->B-->C"))
(print svg)  ;; => SVG string

(mermaid/render-to-file "flowchart TD; X-->Y-->Z" "diagram.svg")
```

## Primitives

| Name | Args | Returns |
|------|------|---------|
| `mermaid/render` | diagram | SVG string |
| `mermaid/render-to-file` | diagram, path | path string |
