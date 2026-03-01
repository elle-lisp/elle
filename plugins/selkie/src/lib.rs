//! Elle selkie plugin — Mermaid diagram rendering via the `selkie-rs` crate.

use elle::effects::Effect;
use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};
use std::collections::BTreeMap;
use std::fs;

/// Plugin entry point. Called by Elle when loading the `.so`.
#[no_mangle]
/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`. The caller must pass a valid
/// `PluginContext` reference. Only safe when called from `load_plugin`.
pub unsafe extern "C" fn elle_plugin_init(ctx: &mut PluginContext) -> Value {
    let mut fields = BTreeMap::new();
    for def in PRIMITIVES {
        ctx.register(def);
        let short_name = def.name.strip_prefix("selkie/").unwrap_or(def.name);
        fields.insert(
            TableKey::Keyword(short_name.into()),
            Value::native_fn(def.func),
        );
    }
    Value::struct_from(fields)
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_selkie_render(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("selkie/render: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    let diagram = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "selkie/render: expected string, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let parsed = match selkie::parse(&diagram) {
        Ok(d) => d,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("selkie-error", format!("selkie/render: parse: {}", e)),
            );
        }
    };
    match selkie::render(&parsed) {
        Ok(svg) => (SIG_OK, Value::string(&*svg)),
        Err(e) => (
            SIG_ERROR,
            error_val("selkie-error", format!("selkie/render: render: {}", e)),
        ),
    }
}

fn prim_selkie_render_to_file(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "selkie/render-to-file: expected 2 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let diagram = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "selkie/render-to-file: expected string, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let path = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "selkie/render-to-file: expected string, got {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    };
    let parsed = match selkie::parse(&diagram) {
        Ok(d) => d,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val(
                    "selkie-error",
                    format!("selkie/render-to-file: parse: {}", e),
                ),
            );
        }
    };
    let svg = match selkie::render(&parsed) {
        Ok(svg) => svg,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val(
                    "selkie-error",
                    format!("selkie/render-to-file: render: {}", e),
                ),
            );
        }
    };
    match fs::write(&path, &svg) {
        Ok(()) => (SIG_OK, Value::string(&*path)),
        Err(e) => (
            SIG_ERROR,
            error_val("io-error", format!("selkie/render-to-file: {}", e)),
        ),
    }
}

fn prim_selkie_render_ascii(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "selkie/render-ascii: expected 1 argument, got {}",
                    args.len()
                ),
            ),
        );
    }
    let diagram = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "selkie/render-ascii: expected string, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let parsed = match selkie::parse(&diagram) {
        Ok(d) => d,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("selkie-error", format!("selkie/render-ascii: parse: {}", e)),
            );
        }
    };
    match selkie::render_ascii(&parsed) {
        Ok(ascii) => (SIG_OK, Value::string(&*ascii)),
        Err(e) => (
            SIG_ERROR,
            error_val(
                "selkie-error",
                format!("selkie/render-ascii: render: {}", e),
            ),
        ),
    }
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "selkie/render",
        func: prim_selkie_render,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Render a Mermaid diagram to SVG",
        params: &["diagram"],
        category: "selkie",
        example: r#"(selkie/render "flowchart LR; A-->B-->C")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "selkie/render-to-file",
        func: prim_selkie_render_to_file,
        effect: Effect::raises(),
        arity: Arity::Exact(2),
        doc: "Render a Mermaid diagram to an SVG file",
        params: &["diagram", "path"],
        category: "selkie",
        example: r#"(selkie/render-to-file "flowchart LR; A-->B" "out.svg")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "selkie/render-ascii",
        func: prim_selkie_render_ascii,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Render a Mermaid diagram to ASCII art",
        params: &["diagram"],
        category: "selkie",
        example: r#"(selkie/render-ascii "flowchart LR; A-->B-->C")"#,
        aliases: &[],
    },
];
