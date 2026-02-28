//! Elle mermaid plugin — Mermaid diagram rendering via the `mermaid-rs-renderer` crate.

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
        let short_name = def.name.strip_prefix("mermaid/").unwrap_or(def.name);
        fields.insert(
            TableKey::Keyword(short_name.into()),
            Value::native_fn(def.func),
        );
    }
    Value::struct_from(fields)
}

// ---------------------------------------------------------------------------
// SVG sanitization
// ---------------------------------------------------------------------------

/// Fix malformed SVG produced by the mermaid renderer.
///
/// The upstream renderer emits `font-family` attributes with unescaped inner
/// quotes, e.g. `font-family="Inter, -apple-system, "Segoe UI", sans-serif"`.
/// The inner `"Segoe UI"` breaks XML parsing. We replace the quoted font name
/// with single quotes, which is valid in both CSS and SVG.
///
// NOTE: callers of `mermaid_rs_renderer::render` depend on this sanitization.
// If you remove it, the SVG output will contain invalid XML.
fn sanitize_svg(svg: &str) -> String {
    // The renderer embeds CSS-style quoted font names directly into XML
    // attributes without escaping. Replace `"FontName"` with `'FontName'`
    // where they appear inside font-family values.
    svg.replace(r#""Segoe UI""#, "'Segoe UI'")
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_mermaid_render(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("mermaid/render: expected 1 argument, got {}", args.len()),
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
                        "mermaid/render: expected string, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    match mermaid_rs_renderer::render(&diagram) {
        Ok(svg) => {
            let svg = sanitize_svg(&svg);
            (SIG_OK, Value::string(&*svg))
        }
        Err(e) => (
            SIG_ERROR,
            error_val("mermaid-error", format!("mermaid/render: {}", e)),
        ),
    }
}

fn prim_mermaid_render_to_file(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "mermaid/render-to-file: expected 2 arguments, got {}",
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
                        "mermaid/render-to-file: expected string, got {}",
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
                        "mermaid/render-to-file: expected string, got {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    };
    let svg = match mermaid_rs_renderer::render(&diagram) {
        Ok(svg) => sanitize_svg(&svg),
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("mermaid-error", format!("mermaid/render-to-file: {}", e)),
            );
        }
    };
    match fs::write(&path, &svg) {
        Ok(()) => (SIG_OK, Value::string(&*path)),
        Err(e) => (
            SIG_ERROR,
            error_val("io-error", format!("mermaid/render-to-file: {}", e)),
        ),
    }
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "mermaid/render",
        func: prim_mermaid_render,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Render a Mermaid diagram to SVG",
        params: &["diagram"],
        category: "mermaid",
        example: r#"(mermaid/render "flowchart LR; A-->B-->C")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "mermaid/render-to-file",
        func: prim_mermaid_render_to_file,
        effect: Effect::raises(),
        arity: Arity::Exact(2),
        doc: "Render a Mermaid diagram to an SVG file",
        params: &["diagram", "path"],
        category: "mermaid",
        example: r#"(mermaid/render-to-file "flowchart LR; A-->B" "out.svg")"#,
        aliases: &[],
    },
];
