//! Filesystem watch primitives — event-driven via inotify (Linux) / kqueue (macOS).

use crate::io::request::{IoOp, IoRequest};
use crate::io::watch::FsWatcher;
use crate::primitives::def::PrimitiveDef;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_IO, SIG_OK, SIG_YIELD};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// (watch) — create a filesystem watcher, returns an External handle.
fn prim_watch(args: &[Value]) -> (SignalBits, Value) {
    if !args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("watch: expected 0 arguments, got {}", args.len()),
            ),
        );
    }
    match FsWatcher::new() {
        Ok(w) => (SIG_OK, Value::external("fs-watcher", w)),
        Err(msg) => (SIG_ERROR, error_val("io-error", msg)),
    }
}

/// (watch-add watcher path) or (watch-add watcher path {:recursive bool})
fn prim_watch_add(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 || args.len() > 3 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("watch-add: expected 2-3 arguments, got {}", args.len()),
            ),
        );
    }
    let watcher = match args[0].as_external::<FsWatcher>() {
        Some(w) => w,
        None => {
            return (
                SIG_ERROR,
                error_val("type-error", "watch-add: first argument must be a watcher"),
            )
        }
    };
    let path = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
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
                s.get(&crate::value::heap::TableKey::Keyword("recursive".into()))
                    .map(|v| v.is_truthy())
            })
            .unwrap_or(true)
    } else {
        true
    };
    match watcher.add(&path, recursive) {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(msg) => (SIG_ERROR, error_val("io-error", msg)),
    }
}

/// (watch-remove watcher path)
fn prim_watch_remove(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("watch-remove: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let watcher = match args[0].as_external::<FsWatcher>() {
        Some(w) => w,
        None => {
            return (
                SIG_ERROR,
                error_val(
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
                error_val(
                    "type-error",
                    "watch-remove: second argument must be a string path",
                ),
            )
        }
    };
    match watcher.remove(&path) {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(msg) => (SIG_ERROR, error_val("io-error", msg)),
    }
}

/// (watch-next watcher) — async: yields SIG_IO, resumes with event batch.
fn prim_watch_next(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("watch-next: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    // Validate it's a watcher before yielding
    if args[0].as_external::<FsWatcher>().is_none() {
        return (
            SIG_ERROR,
            error_val("type-error", "watch-next: argument must be a watcher"),
        );
    }
    (SIG_YIELD | SIG_IO, IoRequest::new(IoOp::WatchNext, args[0]))
}

/// (watch-close watcher)
fn prim_watch_close(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("watch-close: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let watcher = match args[0].as_external::<FsWatcher>() {
        Some(w) => w,
        None => {
            return (
                SIG_ERROR,
                error_val("type-error", "watch-close: argument must be a watcher"),
            )
        }
    };
    watcher.close();
    (SIG_OK, Value::NIL)
}

pub(crate) const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "watch",
        func: prim_watch,
        signal: Signal::errors(),
        arity: Arity::Exact(0),
        doc: "Create a filesystem watcher. Returns a watcher handle for use with watch-add, watch-next, watch-close.",
        params: &[],
        category: "watch",
        example: "(def w (watch))",
        aliases: &[],
    },
    PrimitiveDef {
        name: "watch-add",
        func: prim_watch_add,
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Add a path to the watcher. Recursive by default. Optional third arg: {:recursive false}.",
        params: &["watcher", "path", "opts?"],
        category: "watch",
        example: "(watch-add w \"src/\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "watch-remove",
        func: prim_watch_remove,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Remove a watched path from the watcher.",
        params: &["watcher", "path"],
        category: "watch",
        example: "(watch-remove w \"src/\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "watch-next",
        func: prim_watch_next,
        signal: Signal {
            bits: SignalBits::new(SIG_ERROR.0 | SIG_YIELD.0 | SIG_IO.0),
            propagates: 0,
        },
        arity: Arity::Exact(1),
        doc: "Wait for filesystem events. Yields to the scheduler; resumes with an array of event structs [{:kind :modify :path \"...\"}].",
        params: &["watcher"],
        category: "watch",
        example: "(watch-next w)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "watch-close",
        func: prim_watch_close,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Close the watcher and release its resources.",
        params: &["watcher"],
        category: "watch",
        example: "(watch-close w)",
        aliases: &[],
    },
];
