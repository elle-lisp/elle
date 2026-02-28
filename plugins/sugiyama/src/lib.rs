//! Elle sugiyama plugin — hierarchical graph layout via the `rust-sugiyama` crate.

use elle::effects::Effect;
use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};
use rust_sugiyama::configure::Config;
use std::collections::BTreeMap;

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
        let short_name = def.name.strip_prefix("sugiyama/").unwrap_or(def.name);
        fields.insert(
            TableKey::Keyword(short_name.into()),
            Value::native_fn(def.func),
        );
    }
    Value::struct_from(fields)
}

// ---------------------------------------------------------------------------
// Graph data extraction
// ---------------------------------------------------------------------------

/// Node: (id, label, width, height).
struct Node {
    id: u32,
    label: String,
    width: f64,
    height: f64,
}

/// Extract nodes from a list of lists: ((id "label") (id "label" width height) ...).
///
/// Each element is a list where:
/// - first element is the integer node id
/// - second element is the string label
/// - optional third element is width (defaults to label.len() * 8 + 20)
/// - optional fourth element is height (defaults to 30)
fn extract_nodes(val: &Value, prim_name: &str) -> Result<Vec<Node>, (SignalBits, Value)> {
    let items = val.list_to_vec().map_err(|_| {
        (
            SIG_ERROR,
            error_val("type-error", format!("{prim_name}: nodes must be a list")),
        )
    })?;

    let mut nodes = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let parts = item.list_to_vec().map_err(|_| {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{prim_name}: node {i} must be a list (id \"label\" [width height])"),
                ),
            )
        })?;
        if parts.len() < 2 {
            return Err((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{prim_name}: node {i} must have at least (id \"label\")"),
                ),
            ));
        }
        let id = parts[0].as_int().ok_or_else(|| {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{prim_name}: node {i} id must be an integer"),
                ),
            )
        })? as u32;
        let label = parts[1].with_string(|s| s.to_string()).ok_or_else(|| {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{prim_name}: node {i} label must be a string"),
                ),
            )
        })?;
        let width = if parts.len() > 2 {
            parts[2].as_number().ok_or_else(|| {
                (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("{prim_name}: node {i} width must be a number"),
                    ),
                )
            })?
        } else {
            label.len() as f64 * 8.0 + 20.0
        };
        let height = if parts.len() > 3 {
            parts[3].as_number().ok_or_else(|| {
                (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("{prim_name}: node {i} height must be a number"),
                    ),
                )
            })?
        } else {
            30.0
        };
        nodes.push(Node {
            id,
            label,
            width,
            height,
        });
    }
    Ok(nodes)
}

/// Extract edges from a list of lists: ((from to) (from to) ...).
fn extract_edges(val: &Value, prim_name: &str) -> Result<Vec<(u32, u32)>, (SignalBits, Value)> {
    let items = val.list_to_vec().map_err(|_| {
        (
            SIG_ERROR,
            error_val("type-error", format!("{prim_name}: edges must be a list")),
        )
    })?;

    let mut edges = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let parts = item.list_to_vec().map_err(|_| {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{prim_name}: edge {i} must be a list (from to)"),
                ),
            )
        })?;
        if parts.len() != 2 {
            return Err((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{prim_name}: edge {i} must be a list of exactly 2 elements (from to)"),
                ),
            ));
        }
        let from = parts[0].as_int().ok_or_else(|| {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{prim_name}: edge {i} 'from' must be an integer"),
                ),
            )
        })? as u32;
        let to = parts[1].as_int().ok_or_else(|| {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{prim_name}: edge {i} 'to' must be an integer"),
                ),
            )
        })? as u32;
        edges.push((from, to));
    }
    Ok(edges)
}

/// A positioned vertex: (vertex_index, (x, y)).
type Position = (usize, (f64, f64));

/// Run the Sugiyama layout algorithm. Returns (positions, width, height).
///
/// Positions are merged across all disjoint subgraphs into a single list.
fn compute_layout(nodes: &[Node], edges: &[(u32, u32)]) -> (Vec<Position>, f64, f64) {
    let vertices: Vec<(u32, (f64, f64))> =
        nodes.iter().map(|n| (n.id, (n.width, n.height))).collect();

    let config = Config {
        vertex_spacing: 30.0,
        ..Config::default()
    };

    let subgraphs = rust_sugiyama::from_vertices_and_edges(&vertices, edges, &config);

    // Merge subgraphs: offset each subgraph horizontally.
    let mut all_positions = Vec::new();
    let mut total_width = 0.0_f64;
    let mut max_height = 0.0_f64;
    let gap = 40.0;

    for (positions, w, h) in &subgraphs {
        for &(id, (x, y)) in positions {
            all_positions.push((id, (x + total_width, y)));
        }
        total_width += w + gap;
        max_height = max_height.max(*h);
    }
    // Remove trailing gap.
    if !subgraphs.is_empty() {
        total_width -= gap;
    }

    (all_positions, total_width, max_height)
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_sugiyama_layout(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("sugiyama/layout: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let nodes = match extract_nodes(&args[0], "sugiyama/layout") {
        Ok(n) => n,
        Err(e) => return e,
    };
    let edges = match extract_edges(&args[1], "sugiyama/layout") {
        Ok(e) => e,
        Err(e) => return e,
    };

    let (positions, width, height) = compute_layout(&nodes, &edges);

    let position_values: Vec<Value> = positions
        .iter()
        .map(|&(id, (x, y))| {
            let mut fields = BTreeMap::new();
            fields.insert(TableKey::Keyword("id".into()), Value::int(id as i64));
            fields.insert(TableKey::Keyword("x".into()), Value::float(x));
            fields.insert(TableKey::Keyword("y".into()), Value::float(y));
            Value::struct_from(fields)
        })
        .collect();

    let mut result = BTreeMap::new();
    result.insert(
        TableKey::Keyword("positions".into()),
        elle::list(position_values),
    );
    result.insert(TableKey::Keyword("width".into()), Value::float(width));
    result.insert(TableKey::Keyword("height".into()), Value::float(height));
    (SIG_OK, Value::struct_from(result))
}

fn prim_sugiyama_render(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("sugiyama/render: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let nodes = match extract_nodes(&args[0], "sugiyama/render") {
        Ok(n) => n,
        Err(e) => return e,
    };
    let edges = match extract_edges(&args[1], "sugiyama/render") {
        Ok(e) => e,
        Err(e) => return e,
    };

    let (positions, width, height) = compute_layout(&nodes, &edges);
    let svg = render_svg(&nodes, &positions, &edges, width, height);
    (SIG_OK, Value::string(&*svg))
}

// ---------------------------------------------------------------------------
// SVG rendering
// ---------------------------------------------------------------------------

fn render_svg(
    nodes: &[Node],
    positions: &[(usize, (f64, f64))],
    edges: &[(u32, u32)],
    width: f64,
    height: f64,
) -> String {
    let padding = 40.0;
    let total_w = width + padding * 2.0;
    let total_h = height + padding * 2.0;

    let mut svg = format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {total_w} {total_h}" width="{total_w}" height="{total_h}">"#,
    );

    svg.push_str(
        r#"<style>
.node rect { fill: #dae8fc; stroke: #6c8ebf; stroke-width: 1.5; rx: 4; }
.node text { font-family: sans-serif; font-size: 12px; text-anchor: middle; dominant-baseline: central; }
.edge line { stroke: #333; stroke-width: 1.5; marker-end: url(#arrow); }
</style>"#,
    );

    svg.push_str(
        r##"<defs><marker id="arrow" viewBox="0 0 10 10" refX="10" refY="5" markerWidth="6" markerHeight="6" orient="auto-start-reverse"><path d="M 0 0 L 10 5 L 0 10 z" fill="#333"/></marker></defs>"##,
    );

    // Build position lookup: vertex index -> (x, y).
    let mut pos_map: BTreeMap<usize, (f64, f64)> = BTreeMap::new();
    for &(id, (x, y)) in positions {
        pos_map.insert(id, (x + padding, y + padding));
    }

    // Build node lookup: node id -> Node index.
    let mut node_by_id: BTreeMap<u32, usize> = BTreeMap::new();
    for (i, node) in nodes.iter().enumerate() {
        node_by_id.insert(node.id, i);
    }

    // Draw edges.
    for &(from, to) in edges {
        let from_idx = match node_by_id.get(&from) {
            Some(&i) => i,
            None => continue,
        };
        let to_idx = match node_by_id.get(&to) {
            Some(&i) => i,
            None => continue,
        };
        let (fx, fy) = match pos_map.get(&from_idx) {
            Some(&p) => p,
            None => continue,
        };
        let (tx, ty) = match pos_map.get(&to_idx) {
            Some(&p) => p,
            None => continue,
        };
        let from_node = &nodes[from_idx];
        let to_node = &nodes[to_idx];
        // Edge from bottom-center of source to top-center of target.
        let x1 = fx + from_node.width / 2.0;
        let y1 = fy + from_node.height;
        let x2 = tx + to_node.width / 2.0;
        let y2 = ty;
        svg.push_str(&format!(
            r#"<g class="edge"><line x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}"/></g>"#,
        ));
    }

    // Draw nodes.
    for (i, node) in nodes.iter().enumerate() {
        let (x, y) = match pos_map.get(&i) {
            Some(&p) => p,
            None => continue,
        };
        let label = xml_escape(&node.label);
        svg.push_str(&format!(
            r#"<g class="node"><rect x="{x}" y="{y}" width="{}" height="{}"/><text x="{}" y="{}">{label}</text></g>"#,
            node.width,
            node.height,
            x + node.width / 2.0,
            y + node.height / 2.0,
        ));
    }

    svg.push_str("</svg>");
    svg
}

/// Escape XML special characters in text content.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "sugiyama/layout",
        func: prim_sugiyama_layout,
        effect: Effect::raises(),
        arity: Arity::Exact(2),
        doc: "Compute hierarchical graph layout positions",
        params: &["nodes", "edges"],
        category: "sugiyama",
        example: r#"(sugiyama/layout '((0 "A") (1 "B") (2 "C")) '((0 1) (1 2)))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "sugiyama/render",
        func: prim_sugiyama_render,
        effect: Effect::raises(),
        arity: Arity::Exact(2),
        doc: "Compute hierarchical graph layout and render to SVG string",
        params: &["nodes", "edges"],
        category: "sugiyama",
        example: r#"(sugiyama/render '((0 "A") (1 "B") (2 "C")) '((0 1) (1 2)))"#,
        aliases: &[],
    },
];
