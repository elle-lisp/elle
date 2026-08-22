//! Filesystem watch primitives — event-driven via inotify (Linux) / kqueue (macOS).

use crate::io::request::{IoOp, IoRequest};
use crate::io::watch::FsWatcher;
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK, SIG_YIELD};
use crate::value::types::Arity;
use crate::value::{sorted_struct_get, Value};

/// (watch) — create a filesystem watcher, returns an External handle.
fn prim_watch(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    match FsWatcher::new() {
        Ok(w) => (SIG_OK, ctx.external("fs-watcher", w)),
        Err(msg) => (SIG_ERROR, ctx.error("io-error", msg)),
    }
}

/// (watch-add watcher path) or (watch-add watcher path {:recursive bool})
fn prim_watch_add(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let watcher = match args[0].as_external::<FsWatcher>() {
        Some(w) => w,
        None => {
            return (
                SIG_ERROR,
                ctx.error("type-error", "watch-add: first argument must be a watcher"),
            )
        }
    };
    let path = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    "watch-add: second argument must be a string path",
                ),
            )
        }
    };
    let recursive = if args.len() > 2 {
        // Check for {:recursive bool} struct
        args[2]
            .as_struct()
            .and_then(|s| {
                sorted_struct_get(
                    s,
                    &crate::value::heap::TableKey::Keyword("recursive".into()),
                )
                .map(|v| v.is_truthy())
            })
            .unwrap_or(true)
    } else {
        true
    };
    match watcher.add(&path, recursive) {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(msg) => (SIG_ERROR, ctx.error("io-error", msg)),
    }
}

/// (watch-remove watcher path)
fn prim_watch_remove(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let watcher = match args[0].as_external::<FsWatcher>() {
        Some(w) => w,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    "watch-remove: first argument must be a watcher",
                ),
            )
        }
    };
    let path = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    "watch-remove: second argument must be a string path",
                ),
            )
        }
    };
    match watcher.remove(&path) {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(msg) => (SIG_ERROR, ctx.error("io-error", msg)),
    }
}

/// (watch-next watcher) — async: yields SIG_IO, resumes with event batch.
fn prim_watch_next(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Validate it's a watcher before yielding
    if args[0].as_external::<FsWatcher>().is_none() {
        return (
            SIG_ERROR,
            ctx.error("type-error", "watch-next: argument must be a watcher"),
        );
    }
    (
        SIG_YIELD | SIG_IO,
        IoRequest::new(ctx, IoOp::WatchNext, args[0]),
    )
}

/// (watch-close watcher)
fn prim_watch_close(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    let watcher = match args[0].as_external::<FsWatcher>() {
        Some(w) => w,
        None => {
            return (
                SIG_ERROR,
                ctx.error("type-error", "watch-close: argument must be a watcher"),
            )
        }
    };
    watcher.close();
    (SIG_OK, Value::NIL)
}

primitive! {
    "watch" => prim_watch {
        signal: Signal::errors(),
        doc: "Create a filesystem watcher. Returns a watcher handle for use with watch-add, watch-next, watch-close.",
        category: "watch",
        example: "(def w (watch))",
        effect: RegionEffect::Fresh,
    }
    "watch-add" => prim_watch_add {
        signal: Signal::fs_errors(),
        arity: Arity::Range(2, 3),
        doc: "Add a path to the watcher. Recursive by default. Optional third arg: {:recursive false}.",
        params: &["watcher", "path", "opts?"],
        category: "watch",
        example: "(watch-add w \"src/\")",
        effect: RegionEffect::Immediate,
    }
    "watch-remove" => prim_watch_remove {
        signal: Signal::fs_errors(),
        arity: Arity::Exact(2),
        doc: "Remove a watched path from the watcher.",
        params: &["watcher", "path"],
        category: "watch",
        example: "(watch-remove w \"src/\")",
        effect: RegionEffect::Immediate,
    }
    "watch-next" => prim_watch_next {
        signal: Signal::io_yields_errors(),
        arity: Arity::Exact(1),
        doc: "Wait for filesystem events. Yields to the scheduler; resumes with an array of event structs [{:kind :modify :path \"...\"}]. Event kinds: :create, :modify, :remove, :rename. On macOS (kqueue), :create is reported as :modify because kqueue does not distinguish them at the directory level.",
        params: &["watcher"],
        category: "watch",
        example: "(watch-next w)",
        // Opaque: stores nothing, but the event array is minted at completion on
        // the origin heap (`Alloc::new(completion_heap_ptr(..))`, like sig-next /
        // sys/resolve), neither this call's region nor an arg's. No clique,
        // non-fresh result. (NOT Fresh: no caller-region buffer is pre-allocated.)
        effect: RegionEffect::Opaque,
    }
    "watch-close" => prim_watch_close {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Close the watcher and release its resources.",
        params: &["watcher"],
        category: "watch",
        example: "(watch-close w)",
        effect: RegionEffect::Immediate,
    }
}
