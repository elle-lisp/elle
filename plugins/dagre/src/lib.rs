//! Elle dagre plugin — hierarchical graph layout via the `dagre-rs` crate.

use dagre_rs::{DagreLayout, LayoutOptions, RankDir};
use elle::effects::Effect;
use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};
use petgraph::Graph;
use std::collections::{BTreeMap, HashMap};

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
        let short_name = def.name.strip_prefix("dagre/").unwrap_or(def.name);
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

// ---------------------------------------------------------------------------
// Layout computation
// ---------------------------------------------------------------------------

type Position = (u32, (f64, f64));

/// Run the dagre layout algorithm. Returns (positions, width, height).
///
/// Positions are keyed by the original u32 node id.
fn compute_layout(
    nodes: &[Node],
    edges: &[(u32, u32)],
) -> Result<(Vec<Position>, f64, f64), String> {
    let mut graph = Graph::new();
    let mut id_to_idx: HashMap<u32, petgraph::graph::NodeIndex> = HashMap::new();

    for node in nodes {
        let idx = graph.add_node(node.label.clone());
        id_to_idx.insert(node.id, idx);
    }

    for &(from, to) in edges {
        let from_idx = id_to_idx
            .get(&from)
            .ok_or_else(|| format!("unknown node id {from} in edge"))?;
        let to_idx = id_to_idx
            .get(&to)
            .ok_or_else(|| format!("unknown node id {to} in edge"))?;
        graph.add_edge(*from_idx, *to_idx, ());
    }

    let layout = DagreLayout::with_options(LayoutOptions {
        rank_dir: RankDir::TopToBottom,
        node_sep: 40.0,
        rank_sep: 60.0,
        ..Default::default()
    });

    let result = layout.compute(&graph);

    // Build reverse map: NodeIndex -> u32 id.
    let idx_to_id: HashMap<petgraph::graph::NodeIndex, u32> =
        id_to_idx.iter().map(|(&id, &idx)| (idx, id)).collect();

    let positions: Vec<(u32, (f64, f64))> = result
        .node_positions
        .iter()
        .filter_map(|(idx, &(x, y))| idx_to_id.get(idx).map(|&id| (id, (x as f64, y as f64))))
        .collect();

    Ok((positions, result.width as f64, result.height as f64))
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_dagre_layout(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("dagre/layout: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let nodes = match extract_nodes(&args[0], "dagre/layout") {
        Ok(n) => n,
        Err(e) => return e,
    };
    let edges = match extract_edges(&args[1], "dagre/layout") {
        Ok(e) => e,
        Err(e) => return e,
    };

    let (positions, width, height) = match compute_layout(&nodes, &edges) {
        Ok(r) => r,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("dagre-error", format!("dagre/layout: {e}")),
            );
        }
    };

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

fn prim_dagre_render(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("dagre/render: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let nodes = match extract_nodes(&args[0], "dagre/render") {
        Ok(n) => n,
        Err(e) => return e,
    };
    let edges = match extract_edges(&args[1], "dagre/render") {
        Ok(e) => e,
        Err(e) => return e,
    };

    let (positions, width, height) = match compute_layout(&nodes, &edges) {
        Ok(r) => r,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("dagre-error", format!("dagre/render: {e}")),
            );
        }
    };

    let svg = render_svg(&nodes, &positions, &edges, width, height);
    (SIG_OK, Value::string(&*svg))
}

// ---------------------------------------------------------------------------
// SVG rendering
// ---------------------------------------------------------------------------

fn render_svg(
    nodes: &[Node],
    positions: &[(u32, (f64, f64))],
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

    // Build position lookup: node id -> (x, y) with padding.
    let mut pos_map: HashMap<u32, (f64, f64)> = HashMap::new();
    for &(id, (x, y)) in positions {
        pos_map.insert(id, (x + padding, y + padding));
    }

    // Build node lookup: node id -> &Node.
    let node_by_id: HashMap<u32, &Node> = nodes.iter().map(|n| (n.id, n)).collect();

    // Draw edges.
    for &(from, to) in edges {
        let (fx, fy) = match pos_map.get(&from) {
            Some(&p) => p,
            None => continue,
        };
        let (tx, ty) = match pos_map.get(&to) {
            Some(&p) => p,
            None => continue,
        };
        let from_node = match node_by_id.get(&from) {
            Some(n) => n,
            None => continue,
        };
        let to_node = match node_by_id.get(&to) {
            Some(n) => n,
            None => continue,
        };
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
    for node in nodes {
        let (x, y) = match pos_map.get(&node.id) {
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
        name: "dagre/layout",
        func: prim_dagre_layout,
        effect: Effect::raises(),
        arity: Arity::Exact(2),
        doc: "Compute hierarchical graph layout positions using dagre",
        params: &["nodes", "edges"],
        category: "dagre",
        example: r#"(dagre/layout '((0 "A") (1 "B") (2 "C")) '((0 1) (1 2)))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "dagre/render",
        func: prim_dagre_render,
        effect: Effect::raises(),
        arity: Arity::Exact(2),
        doc: "Compute hierarchical graph layout and render to SVG string using dagre",
        params: &["nodes", "edges"],
        category: "dagre",
        example: r#"(dagre/render '((0 "A") (1 "B") (2 "C")) '((0 1) (1 2)))"#,
        aliases: &[],
    },
];
