//! Elle SVG plugin — SVG rasterization via resvg.
//!
//! Renders SVG strings (or struct trees emitted to XML) to PNG or raw pixels.
//! Construction and emission live in lib/svg.lisp (pure Elle).

use std::collections::BTreeMap;

use elle::primitives::def::PrimitiveDef;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::{Arity, TableKey};
use elle::value::{error_val, Value};

use elle::signals::Signal;

elle::elle_plugin_init!(PRIMITIVES, "svg/");

// ── Helpers ─────────────────────────────────────────────────────────

/// Emit an element struct tree to an SVG XML string.
fn emit_element(val: &Value, out: &mut String) {
    if let Some(s) = val.with_string(|s| s.to_owned()) {
        xml_escape(&s, out);
        return;
    }
    let s = match val.as_struct() {
        Some(s) => s,
        None => return,
    };
    let tag = match elle::value::sorted_struct_get(s, &TableKey::Keyword("tag".into())) {
        Some(v) => match v.as_keyword_name() {
            Some(name) => name,
            None => return,
        },
        None => return,
    };
    out.push('<');
    out.push_str(&tag);
    if let Some(attrs_val) = elle::value::sorted_struct_get(s, &TableKey::Keyword("attrs".into())) {
        if let Some(attrs) = attrs_val.as_struct() {
            for (k, v) in attrs.iter() {
                let key = match k {
                    TableKey::Keyword(name) => name.clone(),
                    TableKey::String(name) => name.clone(),
                    _ => continue,
                };
                out.push(' ');
                out.push_str(&key);
                out.push_str("=\"");
                let val_str = if let Some(s) = v.with_string(|s| s.to_owned()) {
                    s
                } else if let Some(i) = v.as_int() {
                    i.to_string()
                } else if let Some(f) = v.as_float() {
                    format!("{}", f)
                } else if let Some(kw) = v.as_keyword_name() {
                    kw
                } else {
                    continue;
                };
                xml_escape_attr(&val_str, out);
                out.push('"');
            }
        }
    }
    let children = elle::value::sorted_struct_get(s, &TableKey::Keyword("children".into()))
        .and_then(|v| v.as_array());
    match children {
        Some([]) => out.push_str("/>"),
        Some(kids) => {
            out.push('>');
            for child in kids {
                emit_element(child, out);
            }
            out.push_str("</");
            out.push_str(&tag);
            out.push('>');
        }
        None => out.push_str("/>"),
    }
}

fn xml_escape(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(c),
        }
    }
}

fn xml_escape_attr(s: &str, out: &mut String) {
    for c in s.chars() {
        match c {
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            _ => out.push(c),
        }
    }
}

/// Get SVG XML string from either a struct tree or a raw string.
fn get_svg_string(val: &Value, name: &str) -> Result<String, (SignalBits, Value)> {
    if let Some(s) = val.with_string(|s| s.to_owned()) {
        Ok(s)
    } else if val.as_struct().is_some() {
        let mut out = String::new();
        out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>");
        emit_element(val, &mut out);
        Ok(out)
    } else {
        Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected SVG string or struct tree, got {}",
                    name,
                    val.type_name()
                ),
            ),
        ))
    }
}

fn require_string(val: &Value, name: &str, param: &str) -> Result<String, (SignalBits, Value)> {
    val.with_string(|s| s.to_owned()).ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: {} must be string, got {}",
                    name,
                    param,
                    val.type_name()
                ),
            ),
        )
    })
}

// ── Render helpers ──────────────────────────────────────────────────

struct RenderOpts {
    width: Option<u32>,
    height: Option<u32>,
}

fn parse_render_opts(args: &[Value], idx: usize) -> RenderOpts {
    let mut opts = RenderOpts {
        width: None,
        height: None,
    };
    if args.len() > idx {
        if let Some(s) = args[idx].as_struct() {
            if let Some(w) = elle::value::sorted_struct_get(s, &TableKey::Keyword("width".into()))
                .and_then(|v| v.as_int())
            {
                opts.width = Some(w as u32);
            }
            if let Some(h) = elle::value::sorted_struct_get(s, &TableKey::Keyword("height".into()))
                .and_then(|v| v.as_int())
            {
                opts.height = Some(h as u32);
            }
        }
    }
    opts
}

fn render_svg_to_pixmap(
    svg_str: &str,
    opts: &RenderOpts,
) -> Result<resvg::tiny_skia::Pixmap, String> {
    let tree = resvg::usvg::Tree::from_str(svg_str, &resvg::usvg::Options::default())
        .map_err(|e| format!("SVG parse error: {}", e))?;
    let size = tree.size();
    let w = opts.width.unwrap_or(size.width() as u32);
    let h = opts.height.unwrap_or(size.height() as u32);
    let mut pixmap = resvg::tiny_skia::Pixmap::new(w, h)
        .ok_or_else(|| format!("failed to create {}x{} pixmap", w, h))?;
    let sx = w as f32 / size.width();
    let sy = h as f32 / size.height();
    let transform = resvg::tiny_skia::Transform::from_scale(sx, sy);
    resvg::render(&tree, transform, &mut pixmap.as_mut());
    Ok(pixmap)
}

// ── Primitives ──────────────────────────────────────────────────────

fn prim_render(args: &[Value]) -> (SignalBits, Value) {
    let svg_str = match get_svg_string(&args[0], "svg/render") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let opts = parse_render_opts(args, 1);
    match render_svg_to_pixmap(&svg_str, &opts) {
        Ok(pixmap) => match pixmap.encode_png() {
            Ok(png_data) => (SIG_OK, Value::bytes(png_data)),
            Err(e) => (
                SIG_ERROR,
                error_val("svg-error", format!("svg/render: PNG encode: {}", e)),
            ),
        },
        Err(e) => (
            SIG_ERROR,
            error_val("svg-error", format!("svg/render: {}", e)),
        ),
    }
}

fn prim_render_raw(args: &[Value]) -> (SignalBits, Value) {
    let svg_str = match get_svg_string(&args[0], "svg/render-raw") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let opts = parse_render_opts(args, 1);
    match render_svg_to_pixmap(&svg_str, &opts) {
        Ok(pixmap) => {
            let w = pixmap.width();
            let h = pixmap.height();
            let data = pixmap.take();
            let mut fields = BTreeMap::new();
            fields.insert(TableKey::Keyword("width".into()), Value::int(w as i64));
            fields.insert(TableKey::Keyword("height".into()), Value::int(h as i64));
            fields.insert(TableKey::Keyword("data".into()), Value::bytes(data));
            (SIG_OK, Value::struct_from(fields))
        }
        Err(e) => (
            SIG_ERROR,
            error_val("svg-error", format!("svg/render-raw: {}", e)),
        ),
    }
}

fn prim_render_to_file(args: &[Value]) -> (SignalBits, Value) {
    let svg_str = match get_svg_string(&args[0], "svg/render-to-file") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let path = match require_string(&args[1], "svg/render-to-file", "path") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let opts = parse_render_opts(args, 2);
    match render_svg_to_pixmap(&svg_str, &opts) {
        Ok(pixmap) => match pixmap.save_png(&path) {
            Ok(()) => (SIG_OK, Value::NIL),
            Err(e) => (
                SIG_ERROR,
                error_val("svg-error", format!("svg/render-to-file: {}", e)),
            ),
        },
        Err(e) => (
            SIG_ERROR,
            error_val("svg-error", format!("svg/render-to-file: {}", e)),
        ),
    }
}

fn prim_dimensions(args: &[Value]) -> (SignalBits, Value) {
    let svg_str = match get_svg_string(&args[0], "svg/dimensions") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match resvg::usvg::Tree::from_str(&svg_str, &resvg::usvg::Options::default()) {
        Ok(tree) => {
            let size = tree.size();
            (
                SIG_OK,
                Value::array(vec![
                    Value::float(size.width() as f64),
                    Value::float(size.height() as f64),
                ]),
            )
        }
        Err(e) => (
            SIG_ERROR,
            error_val("svg-error", format!("svg/dimensions: {}", e)),
        ),
    }
}

// ── Registration ────────────────────────────────────────────────────

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "svg/render", func: prim_render, signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Render SVG (string or struct tree) to PNG bytes. Optional opts: {:width N :height N}.",
        params: &["source", "opts"], category: "svg",
        example: "(svg/render \"<svg width='100' height='100'><circle cx='50' cy='50' r='40' fill='red'/></svg>\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "svg/render-raw", func: prim_render_raw, signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Render SVG to raw RGBA8 pixels. Returns {:width :height :data bytes}.",
        params: &["source", "opts"], category: "svg",
        example: "(svg/render-raw svg-string)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "svg/render-to-file", func: prim_render_to_file, signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: "Render SVG to a PNG file.",
        params: &["source", "path", "opts"], category: "svg",
        example: "(svg/render-to-file svg-string \"output.png\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "svg/dimensions", func: prim_dimensions, signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return [width height] of an SVG's intrinsic dimensions.",
        params: &["source"], category: "svg",
        example: "(svg/dimensions \"<svg width='100' height='200'></svg>\")",
        aliases: &[],
    },
];
