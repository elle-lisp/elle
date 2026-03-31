//! Elle egui plugin — thin wrapper over egui + winit + glow.
//!
//! Provides window lifecycle and synchronous frame rendering.
//! All I/O awareness (ev/poll-fd) lives in the Elle library.

mod ui;
mod window;

use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::error_val;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{TableKey, Value};
use std::cell::RefCell;
use std::collections::BTreeMap;

use crate::ui::Interactions;
use crate::window::WindowState;

// ── Entry point ──────────────────────────────────────────────────────

#[no_mangle]
/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`.
pub unsafe extern "C" fn elle_plugin_init(ctx: &mut PluginContext) -> Value {
    ctx.init_keywords();
    let mut fields = BTreeMap::new();
    for def in PRIMITIVES {
        ctx.register(def);
        let short = def.name.strip_prefix("egui/").unwrap_or(def.name);
        fields.insert(TableKey::Keyword(short.into()), Value::native_fn(def.func));
    }
    Value::struct_from(fields)
}

// ── Helpers ──────────────────────────────────────────────────────────

fn get_state(val: &Value) -> Result<&RefCell<WindowState>, (SignalBits, Value)> {
    val.as_external::<RefCell<WindowState>>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("expected egui-window handle, got {}", val.type_name()),
            ),
        )
    })
}

fn egui_err(name: &str, msg: impl std::fmt::Display) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val("egui-error", format!("{}: {}", name, msg)),
    )
}

// ── Primitives ───────────────────────────────────────────────────────

/// (egui/open) or (egui/open {:title "My App"})
fn prim_open(args: &[Value]) -> (SignalBits, Value) {
    let mut title = "Elle".to_string();
    let mut width = 800.0;
    let mut height = 600.0;

    if !args.is_empty() {
        if let Some(s) = args[0].as_struct() {
            if let Some(v) = s.get(&TableKey::Keyword("title".into())) {
                if let Some(t) = v.with_string(|s| s.to_string()) {
                    title = t;
                }
            }
            if let Some(v) = s.get(&TableKey::Keyword("width".into())) {
                if let Some(w) = v.as_float().or_else(|| v.as_int().map(|i| i as f64)) {
                    width = w;
                }
            }
            if let Some(v) = s.get(&TableKey::Keyword("height".into())) {
                if let Some(h) = v.as_float().or_else(|| v.as_int().map(|i| i as f64)) {
                    height = h;
                }
            }
        }
    }

    match WindowState::new(&title, width, height) {
        Ok(state) => (SIG_OK, Value::external("egui-window", RefCell::new(state))),
        Err(e) => egui_err("egui/open", e),
    }
}

/// (egui/display-fd handle) → int
fn prim_display_fd(args: &[Value]) -> (SignalBits, Value) {
    let state = match get_state(&args[0]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let fd = state.borrow().display_fd;
    (SIG_OK, Value::int(fd as i64))
}

/// (egui/frame handle tree) → interactions struct
fn prim_frame(args: &[Value]) -> (SignalBits, Value) {
    let state = match get_state(&args[0]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let nodes = match ui::value_to_tree(&args[1]) {
        Ok(n) => n,
        Err(e) => return e,
    };

    let mut state = state.borrow_mut();

    // Pump pending winit events (non-blocking)
    state.pump_events();

    // Render the tree
    let mut ix = Interactions::default();
    state.frame_with_tree(&nodes, &mut ix);

    // Convert interactions to Elle value
    (SIG_OK, ui::interactions_to_value(&ix))
}

/// (egui/close handle)
fn prim_close(args: &[Value]) -> (SignalBits, Value) {
    let state = match get_state(&args[0]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut state = state.borrow_mut();
    state.close_requested = true;
    // Drop GL resources
    state.painter = None;
    state.egui_winit = None;
    state.gl = None;
    state.gl_context = None;
    state.gl_surface = None;
    state.window = None;
    state.event_loop = None;
    (SIG_OK, Value::NIL)
}

/// (egui/open? handle) → bool
fn prim_open_p(args: &[Value]) -> (SignalBits, Value) {
    let state = match get_state(&args[0]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let state = state.borrow();
    (
        SIG_OK,
        Value::bool(!state.close_requested && state.window.is_some()),
    )
}

/// (egui/set-text handle id value)
fn prim_set_text(args: &[Value]) -> (SignalBits, Value) {
    let state = match get_state(&args[0]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let id = match args[1].as_keyword_name() {
        Some(s) => s,
        None => {
            return egui_err(
                "egui/set-text",
                format!("id must be a keyword, got {}", args[1].type_name()),
            )
        }
    };
    let val = match args[2].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return egui_err(
                "egui/set-text",
                format!("value must be a string, got {}", args[2].type_name()),
            )
        }
    };
    state.borrow_mut().widget_state.text_buffers.insert(id, val);
    (SIG_OK, Value::NIL)
}

/// (egui/set-check handle id value)
fn prim_set_check(args: &[Value]) -> (SignalBits, Value) {
    let state = match get_state(&args[0]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let id = match args[1].as_keyword_name() {
        Some(s) => s,
        None => return egui_err("egui/set-check", "id must be a keyword"),
    };
    let val = match args[2].as_bool() {
        Some(b) => b,
        None => return egui_err("egui/set-check", "value must be a boolean"),
    };
    state.borrow_mut().widget_state.check_states.insert(id, val);
    (SIG_OK, Value::NIL)
}

/// (egui/set-slider handle id value)
fn prim_set_slider(args: &[Value]) -> (SignalBits, Value) {
    let state = match get_state(&args[0]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let id = match args[1].as_keyword_name() {
        Some(s) => s,
        None => return egui_err("egui/set-slider", "id must be a keyword"),
    };
    let val = args[2]
        .as_float()
        .or_else(|| args[2].as_int().map(|i| i as f64));
    match val {
        Some(v) => {
            state.borrow_mut().widget_state.slider_states.insert(id, v);
            (SIG_OK, Value::NIL)
        }
        None => egui_err("egui/set-slider", "value must be a number"),
    }
}

/// (egui/set-title handle title)
fn prim_set_title(args: &[Value]) -> (SignalBits, Value) {
    let state = match get_state(&args[0]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let title = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => return egui_err("egui/set-title", "title must be a string"),
    };
    let state = state.borrow();
    if let Some(ref window) = state.window {
        window.set_title(&title);
    }
    (SIG_OK, Value::NIL)
}

/// (egui/set-combo handle id value)
fn prim_set_combo(args: &[Value]) -> (SignalBits, Value) {
    let state = match get_state(&args[0]) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let id = match args[1].as_keyword_name() {
        Some(s) => s,
        None => return egui_err("egui/set-combo", "id must be a keyword"),
    };
    let val = match args[2].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => return egui_err("egui/set-combo", "value must be a string"),
    };
    state.borrow_mut().widget_state.combo_states.insert(id, val);
    (SIG_OK, Value::NIL)
}

// ── Primitive table ──────────────────────────────────────────────────

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "egui/open",
        func: prim_open,
        signal: Signal::errors(),
        arity: Arity::Range(0, 1),
        doc: "Open a GUI window. Optional opts: {:title \"name\"}",
        params: &["opts?"],
        category: "egui",
        example: "(egui/open {:title \"My App\"})",
        aliases: &[],
    },
    PrimitiveDef {
        name: "egui/display-fd",
        func: prim_display_fd,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Return the display connection fd for ev/poll-fd.",
        params: &["handle"],
        category: "egui",
        example: "(egui/display-fd handle)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "egui/frame",
        func: prim_frame,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Render one frame. Pumps events, renders tree, returns interactions.",
        params: &["handle", "tree"],
        category: "egui",
        example: "(egui/frame handle [:label \"hello\"])",
        aliases: &[],
    },
    PrimitiveDef {
        name: "egui/close",
        func: prim_close,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Close the window and release resources.",
        params: &["handle"],
        category: "egui",
        example: "(egui/close handle)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "egui/open?",
        func: prim_open_p,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Check if the window is still open.",
        params: &["handle"],
        category: "egui",
        example: "(egui/open? handle)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "egui/set-text",
        func: prim_set_text,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Set a text input's buffer value.",
        params: &["handle", "id", "value"],
        category: "egui",
        example: "(egui/set-text handle :name \"world\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "egui/set-check",
        func: prim_set_check,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Set a checkbox state.",
        params: &["handle", "id", "checked"],
        category: "egui",
        example: "(egui/set-check handle :agree true)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "egui/set-slider",
        func: prim_set_slider,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Set a slider value.",
        params: &["handle", "id", "value"],
        category: "egui",
        example: "(egui/set-slider handle :volume 50.0)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "egui/set-title",
        func: prim_set_title,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Change the window title.",
        params: &["handle", "title"],
        category: "egui",
        example: "(egui/set-title handle \"New Title\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "egui/set-combo",
        func: prim_set_combo,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Set a combo box selection.",
        params: &["handle", "id", "value"],
        category: "egui",
        example: "(egui/set-combo handle :theme \"dark\")",
        aliases: &[],
    },
];
