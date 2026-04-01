//! Elle watch plugin — filesystem event watching via notify + debouncer.

use std::collections::{BTreeMap, VecDeque};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use notify::RecursiveMode;
use notify_debouncer_mini::{new_debouncer, DebouncedEvent, Debouncer};

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

#[no_mangle]
/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`. The caller must pass a valid
/// `PluginContext` reference. Only safe when called from `load_plugin`.
elle::elle_plugin_init!(PRIMITIVES, "watch/");

// ---------------------------------------------------------------------------
// Watcher state
// ---------------------------------------------------------------------------

type EventQueue = Arc<Mutex<VecDeque<WatchEvent>>>;

struct WatcherState {
    debouncer: Option<Debouncer<notify::RecommendedWatcher>>,
    events: EventQueue,
    _drain_handle: Option<std::thread::JoinHandle<()>>,
}

#[derive(Clone)]
struct WatchEvent {
    kind: EventKind,
    paths: Vec<String>,
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
enum EventKind {
    Create,
    Modify,
    Remove,
    Other,
}

impl EventKind {
    fn as_keyword(&self) -> &'static str {
        match self {
            EventKind::Create => "create",
            EventKind::Modify => "modify",
            EventKind::Remove => "remove",
            EventKind::Other => "other",
        }
    }
}

fn event_to_value(event: &WatchEvent) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(
        TableKey::Keyword("kind".into()),
        Value::keyword(event.kind.as_keyword()),
    );
    let paths: Vec<Value> = event
        .paths
        .iter()
        .map(|p| Value::string(p.as_str()))
        .collect();
    fields.insert(TableKey::Keyword("paths".into()), Value::array(paths));
    Value::struct_from(fields)
}

fn classify_event(event: &DebouncedEvent) -> EventKind {
    let path = &event.path;
    if !path.exists() {
        EventKind::Remove
    } else {
        EventKind::Modify
    }
}

/// Extract a keyword field from a struct value.
fn struct_get_keyword(val: &Value, field: &str) -> Option<Value> {
    let s = val.as_struct()?;
    s.get(&TableKey::Keyword(field.into())).copied()
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

/// watch/new — create a watcher with optional config struct
fn prim_new(args: &[Value]) -> (SignalBits, Value) {
    let debounce_ms: u64 = if args.is_empty() {
        500
    } else {
        match struct_get_keyword(&args[0], "debounce") {
            Some(v) => match v.as_int() {
                Some(ms) => ms as u64,
                None => {
                    return (
                        SIG_ERROR,
                        error_val("type-error", "watch/new: :debounce must be an integer"),
                    )
                }
            },
            None => 500,
        }
    };

    let events: EventQueue = Arc::new(Mutex::new(VecDeque::new()));
    let events_clone = events.clone();

    let (tx, rx): (
        mpsc::Sender<Vec<DebouncedEvent>>,
        Receiver<Vec<DebouncedEvent>>,
    ) = mpsc::channel();

    let debouncer = match new_debouncer(
        Duration::from_millis(debounce_ms),
        move |result: Result<Vec<DebouncedEvent>, notify::Error>| {
            if let Ok(events) = result {
                let _ = tx.send(events);
            }
        },
    ) {
        Ok(d) => d,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val(
                    "io-error",
                    format!("watch/new: failed to create watcher: {e}"),
                ),
            )
        }
    };

    // Background thread: drain channel into event queue
    let drain_handle = std::thread::spawn(move || {
        while let Ok(batch) = rx.recv() {
            let mut queue = events_clone.lock().unwrap();
            for raw in &batch {
                let kind = classify_event(raw);
                let path_str = raw.path.to_string_lossy().to_string();
                queue.push_back(WatchEvent {
                    kind,
                    paths: vec![path_str],
                });
            }
        }
    });

    let state = WatcherState {
        debouncer: Some(debouncer),
        events,
        _drain_handle: Some(drain_handle),
    };

    (SIG_OK, Value::external("watch/watcher", Mutex::new(state)))
}

/// watch/add — add a path to the watcher
fn prim_add(args: &[Value]) -> (SignalBits, Value) {
    if args.len() < 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("watch/add: expected 2-3 arguments, got {}", args.len()),
            ),
        );
    }

    let state_mutex = match args[0].as_external::<Mutex<WatcherState>>() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val("type-error", "watch/add: first argument must be a watcher"),
            )
        }
    };

    let path_str = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "watch/add: second argument must be a string path",
                ),
            )
        }
    };

    let recursive = if args.len() > 2 {
        match struct_get_keyword(&args[2], "recursive") {
            Some(v) => v.is_truthy(),
            None => true,
        }
    } else {
        true
    };

    let mode = if recursive {
        RecursiveMode::Recursive
    } else {
        RecursiveMode::NonRecursive
    };

    let mut state = state_mutex.lock().unwrap();
    match &mut state.debouncer {
        Some(debouncer) => {
            if let Err(e) = debouncer.watcher().watch(&PathBuf::from(&path_str), mode) {
                return (
                    SIG_ERROR,
                    error_val(
                        "io-error",
                        format!("watch/add: failed to watch {path_str}: {e}"),
                    ),
                );
            }
        }
        None => {
            return (
                SIG_ERROR,
                error_val("io-error", "watch/add: watcher is closed"),
            )
        }
    }

    (SIG_OK, Value::NIL)
}

/// watch/remove — remove a watched path
fn prim_remove(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("watch/remove: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let state_mutex = match args[0].as_external::<Mutex<WatcherState>>() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "watch/remove: first argument must be a watcher",
                ),
            )
        }
    };

    let path_str = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    "watch/remove: second argument must be a string path",
                ),
            )
        }
    };

    let mut state = state_mutex.lock().unwrap();
    match &mut state.debouncer {
        Some(debouncer) => {
            if let Err(e) = debouncer.watcher().unwatch(&PathBuf::from(&path_str)) {
                return (
                    SIG_ERROR,
                    error_val(
                        "io-error",
                        format!("watch/remove: failed to unwatch {path_str}: {e}"),
                    ),
                );
            }
        }
        None => {
            return (
                SIG_ERROR,
                error_val("io-error", "watch/remove: watcher is closed"),
            )
        }
    }

    (SIG_OK, Value::NIL)
}

/// watch/next — poll for the next event
fn prim_next(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("watch/next: expected 1-2 arguments, got {}", args.len()),
            ),
        );
    }

    let state_mutex = match args[0].as_external::<Mutex<WatcherState>>() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val("type-error", "watch/next: first argument must be a watcher"),
            )
        }
    };

    let timeout_ms: Option<u64> = if args.len() > 1 {
        match struct_get_keyword(&args[1], "timeout") {
            Some(v) => match v.as_int() {
                Some(ms) => Some(ms as u64),
                None => {
                    return (
                        SIG_ERROR,
                        error_val("type-error", "watch/next: :timeout must be an integer"),
                    )
                }
            },
            None => None,
        }
    } else {
        None
    };

    // Get a clone of the event queue Arc
    let events = {
        let state = state_mutex.lock().unwrap();
        state.events.clone()
    };

    // Try to pop an event
    if let Some(event) = events.lock().unwrap().pop_front() {
        return (SIG_OK, event_to_value(&event));
    }

    // If no timeout, return nil immediately
    let timeout_ms = match timeout_ms {
        Some(ms) => ms,
        None => return (SIG_OK, Value::NIL),
    };

    // Poll with timeout
    let poll_interval = Duration::from_millis(10);
    let deadline = std::time::Instant::now() + Duration::from_millis(timeout_ms);

    loop {
        std::thread::sleep(poll_interval);
        if let Some(event) = events.lock().unwrap().pop_front() {
            return (SIG_OK, event_to_value(&event));
        }
        if std::time::Instant::now() >= deadline {
            return (SIG_OK, Value::NIL);
        }
    }
}

/// watch/close — close the watcher and stop the background thread
fn prim_close(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("watch/close: expected 1 argument, got {}", args.len()),
            ),
        );
    }

    let state_mutex = match args[0].as_external::<Mutex<WatcherState>>() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val("type-error", "watch/close: argument must be a watcher"),
            )
        }
    };

    let mut state = state_mutex.lock().unwrap();
    state.debouncer.take();
    state.events.lock().unwrap().clear();

    (SIG_OK, Value::NIL)
}

// ---------------------------------------------------------------------------
// Primitive table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "watch/new",
        func: prim_new,
        signal: Signal::errors(),
        arity: Arity::Range(0, 1),
        doc: "Create a filesystem watcher. Optional config: {:debounce ms}.",
        params: &["config?"],
        category: "watch",
        example: "(watch/new) or (watch/new {:debounce 200})",
        aliases: &[],
    },
    PrimitiveDef {
        name: "watch/add",
        func: prim_add,
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Add a path to the watcher. Optional config: {:recursive bool}.",
        params: &["watcher", "path", "config?"],
        category: "watch",
        example: "(watch/add w \"lib/\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "watch/remove",
        func: prim_remove,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Remove a watched path from the watcher.",
        params: &["watcher", "path"],
        category: "watch",
        example: "(watch/remove w \"lib/\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "watch/next",
        func: prim_next,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Poll for next event. Non-blocking or with {:timeout ms}.",
        params: &["watcher", "config?"],
        category: "watch",
        example: "(watch/next w) or (watch/next w {:timeout 1000})",
        aliases: &[],
    },
    PrimitiveDef {
        name: "watch/close",
        func: prim_close,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Close the watcher, stop background thread, drain events.",
        params: &["watcher"],
        category: "watch",
        example: "(watch/close w)",
        aliases: &[],
    },
];
