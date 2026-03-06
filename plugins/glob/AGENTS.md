# plugins/glob

Filesystem globbing plugin for Elle.

## Responsibility

Provide filesystem pattern matching primitives:
- Match paths against glob patterns
- Expand glob patterns to matching paths
- Support standard glob syntax (`*`, `?`, `[...]`, `**`)

## Primitives

| Name | Arity | Purpose |
|------|-------|---------|
| `glob/match` | 2 | Test if a path matches a glob pattern |
| `glob/glob` | 1 | Expand a glob pattern to matching paths |

## Implementation

Uses the `glob` crate for pattern matching and filesystem traversal.

## Building

```bash
cd plugins/glob
cargo build --release
# Output: target/release/libelle_glob.so
```

## Loading

```janet
(import "target/release/libelle_glob.so")
(glob/match "*.lisp" "hello.lisp")  ; => true
(glob/glob "examples/*.lisp")       ; => array of matching paths
```

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Package metadata and dependencies |
| `src/lib.rs` | Plugin implementation |
