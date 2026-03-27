# plugins/watch

Filesystem watcher plugin for Elle, wrapping the `notify` crate.

## Responsibility

Provide filesystem event watching primitives:
- Create debounced file watchers
- Add/remove watched paths
- Poll for filesystem events (create, modify, remove, rename)

## Primitives

| Name | Arity | Signal | Purpose |
|------|-------|--------|---------|
| `watch/new` | 0-1 | errors | Create a watcher with optional debounce config |
| `watch/add` | 2-3 | errors | Add a path to watch (recursive by default) |
| `watch/remove` | 2 | errors | Remove a watched path |
| `watch/next` | 1-2 | errors | Poll for next event (non-blocking or with timeout) |
| `watch/close` | 1 | errors | Close watcher and stop background thread |

## Architecture

The plugin manages a background thread internally. `notify`'s
debouncer sends events through a std mpsc channel. Events accumulate
in an `Arc<Mutex<VecDeque>>` shared between the watcher thread and
Elle-facing primitives.

## Building

```bash
cargo build --release -p elle-watch
# Output: target/release/libelle_watch.so
```

## Loading

```lisp
(def w (import "target/release/libelle_watch.so"))
(def watcher (w:new))
(w:add watcher "lib/")
(def event (w:next watcher))
(w:close watcher)
```

## Files

| File | Purpose |
|------|---------|
| `Cargo.toml` | Package metadata and dependencies |
| `src/lib.rs` | Plugin implementation |
