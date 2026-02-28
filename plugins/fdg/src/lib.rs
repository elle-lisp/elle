//! Elle fdg plugin — force-directed graph layout via the `fdg-sim` crate.

use elle::effects::Effect;
use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};
use fdg_sim::{ForceGraph, ForceGraphHelper, Simulation, SimulationParameters};
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;

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
        let short_name = def.name.strip_prefix("fdg/").unwrap_or(def.name);
        fields.insert(
            TableKey::Keyword(short_name.into()),
            Value::native_fn(def.func),
        );
    }
    Value::struct_from(fields)
}

// ---------------------------------------------------------------------------
// Shared types and helpers
// ---------------------------------------------------------------------------

struct LayoutNode {
    id: u32,
    label: String,
    x: f64,
    y: f64,
}

struct LayoutData {
    nodes: Vec<LayoutNode>,
    width: f64,
    height: f64,
}

const PADDING: f64 = 40.0;
const NODE_RADIUS: f64 = 20.0;
const ITERATIONS: usize = 200;
const DT: f32 = 0.035;

/// Extract node data from Elle list-of-lists: `((id "label") ...)`.
fn extract_nodes(val: &Value) -> Result<Vec<(u32, String)>, String> {
    let items = val
        .list_to_vec()
        .map_err(|_| "fdg: nodes must be a list".to_string())?;
    let mut nodes = Vec::with_capacity(items.len());
    for item in &items {
        let pair = item
            .list_to_vec()
            .map_err(|_| "fdg: each node must be a list (id \"label\")".to_string())?;
        if pair.len() != 2 {
            return Err("fdg: each node must be (id \"label\")".into());
        }
        let id = pair[0].as_int().ok_or("fdg: node id must be an integer")? as u32;
        let label = pair[1]
            .with_string(|s| s.to_string())
            .ok_or("fdg: node label must be a string")?;
        nodes.push((id, label));
    }
    Ok(nodes)
}

/// Extract edge data from Elle list-of-lists: `((from to) ...)`.
fn extract_edges(val: &Value) -> Result<Vec<(u32, u32)>, String> {
    let items = val
        .list_to_vec()
        .map_err(|_| "fdg: edges must be a list".to_string())?;
    let mut edges = Vec::with_capacity(items.len());
    for item in &items {
        let pair = item
            .list_to_vec()
            .map_err(|_| "fdg: each edge must be a list (from to)".to_string())?;
        if pair.len() != 2 {
            return Err("fdg: each edge must be (from to)".into());
        }
        let from = pair[0]
            .as_int()
            .ok_or("fdg: edge endpoint must be an integer")? as u32;
        let to = pair[1]
            .as_int()
            .ok_or("fdg: edge endpoint must be an integer")? as u32;
        edges.push((from, to));
    }
    Ok(edges)
}

/// Run force-directed layout and return normalized positions.
fn compute_layout(
    node_data: &[(u32, String)],
    edge_data: &[(u32, u32)],
) -> Result<LayoutData, String> {
    let mut graph: ForceGraph<u32, ()> = ForceGraph::default();
    let mut index_map: HashMap<u32, petgraph::graph::NodeIndex> = HashMap::new();

    for (id, label) in node_data {
        let idx = graph.add_force_node(label, *id);
        index_map.insert(*id, idx);
    }

    for (from, to) in edge_data {
        let from_idx = *index_map
            .get(from)
            .ok_or_else(|| format!("fdg: unknown source node {from}"))?;
        let to_idx = *index_map
            .get(to)
            .ok_or_else(|| format!("fdg: unknown target node {to}"))?;
        graph.add_edge(from_idx, to_idx, ());
    }

    let mut sim = Simulation::from_graph(graph, SimulationParameters::default());

    for _ in 0..ITERATIONS {
        sim.update(DT);
    }

    let graph = sim.get_graph();

    let mut raw_positions: Vec<(u32, String, f64, f64)> = Vec::new();
    let mut min_x = f64::MAX;
    let mut min_y = f64::MAX;
    let mut max_x = f64::MIN;
    let mut max_y = f64::MIN;

    for node in graph.node_weights() {
        let x = node.location.x as f64;
        let y = node.location.y as f64;
        min_x = min_x.min(x);
        min_y = min_y.min(y);
        max_x = max_x.max(x);
        max_y = max_y.max(y);
        raw_positions.push((node.data, node.name.clone(), x, y));
    }

    // Normalize: shift so minimum is at PADDING, scale if needed
    let raw_w = max_x - min_x;
    let raw_h = max_y - min_y;
    let scale = if raw_w < 1.0 && raw_h < 1.0 {
        1.0
    } else {
        // Scale to a reasonable viewport while preserving aspect ratio
        let target = 600.0;
        let s = target / raw_w.max(raw_h);
        if s < 0.1 {
            0.1
        } else {
            s
        }
    };

    let nodes: Vec<LayoutNode> = raw_positions
        .into_iter()
        .map(|(id, label, x, y)| LayoutNode {
            id,
            label,
            x: (x - min_x) * scale + PADDING + NODE_RADIUS,
            y: (y - min_y) * scale + PADDING + NODE_RADIUS,
        })
        .collect();

    let width = raw_w * scale + 2.0 * (PADDING + NODE_RADIUS);
    let height = raw_h * scale + 2.0 * (PADDING + NODE_RADIUS);

    Ok(LayoutData {
        nodes,
        width,
        height,
    })
}

/// Render layout data to an SVG string.
fn render_svg(layout: &LayoutData, edge_data: &[(u32, u32)]) -> String {
    let mut svg = String::with_capacity(4096);

    // Build position lookup by node id
    let pos: HashMap<u32, (f64, f64)> = layout.nodes.iter().map(|n| (n.id, (n.x, n.y))).collect();

    let _ = write!(
        svg,
        r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {w} {h}" width="{w}" height="{h}">
<defs>
  <marker id="arrow" viewBox="0 0 10 6" refX="10" refY="3"
    markerWidth="10" markerHeight="6" orient="auto-start-reverse">
    <path d="M 0 0 L 10 3 L 0 6 z" fill="#333"/>
  </marker>
</defs>
<style>
  .node {{ fill: #4a90d9; stroke: #2c5f8a; stroke-width: 1.5; }}
  .label {{ font-family: sans-serif; font-size: 12px; fill: #fff; text-anchor: middle; dominant-baseline: central; }}
  .edge {{ stroke: #333; stroke-width: 1.5; fill: none; marker-end: url(#arrow); }}
</style>
"##,
        w = layout.width.ceil() as u32,
        h = layout.height.ceil() as u32,
    );

    // Draw edges
    for (from, to) in edge_data {
        if let (Some(&(x1, y1)), Some(&(x2, y2))) = (pos.get(from), pos.get(to)) {
            // Shorten line to stop at node circle edge
            let dx = x2 - x1;
            let dy = y2 - y1;
            let dist = (dx * dx + dy * dy).sqrt();
            if dist > 0.0 {
                let ux = dx / dist;
                let uy = dy / dist;
                let sx = x1 + ux * NODE_RADIUS;
                let sy = y1 + uy * NODE_RADIUS;
                let ex = x2 - ux * NODE_RADIUS;
                let ey = y2 - uy * NODE_RADIUS;
                let _ = writeln!(
                    svg,
                    r#"<line class="edge" x1="{sx:.1}" y1="{sy:.1}" x2="{ex:.1}" y2="{ey:.1}"/>"#,
                );
            }
        }
    }

    // Draw nodes
    for node in &layout.nodes {
        let label = xml_escape(&node.label);
        let _ = writeln!(
            svg,
            r#"<circle class="node" cx="{x:.1}" cy="{y:.1}" r="{r}"/>"#,
            x = node.x,
            y = node.y,
            r = NODE_RADIUS,
        );
        let _ = writeln!(
            svg,
            r#"<text class="label" x="{x:.1}" y="{y:.1}">{label}</text>"#,
            x = node.x,
            y = node.y,
        );
    }

    svg.push_str("</svg>\n");
    svg
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_fdg_layout(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fdg/layout: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let node_data = match extract_nodes(&args[0]) {
        Ok(n) => n,
        Err(e) => return (SIG_ERROR, error_val("type-error", e)),
    };
    let edge_data = match extract_edges(&args[1]) {
        Ok(e) => e,
        Err(e) => return (SIG_ERROR, error_val("type-error", e)),
    };
    let layout = match compute_layout(&node_data, &edge_data) {
        Ok(l) => l,
        Err(e) => return (SIG_ERROR, error_val("fdg-error", e)),
    };

    // Build result struct {:positions <list> :width <num> :height <num>}
    let positions: Vec<Value> = layout
        .nodes
        .iter()
        .map(|n| {
            let mut fields = BTreeMap::new();
            fields.insert(TableKey::Keyword("id".into()), Value::int(n.id as i64));
            fields.insert(TableKey::Keyword("label".into()), Value::string(&*n.label));
            fields.insert(TableKey::Keyword("x".into()), Value::float(n.x));
            fields.insert(TableKey::Keyword("y".into()), Value::float(n.y));
            Value::struct_from(fields)
        })
        .collect();

    let mut result = BTreeMap::new();
    result.insert(TableKey::Keyword("positions".into()), elle::list(positions));
    result.insert(
        TableKey::Keyword("width".into()),
        Value::float(layout.width),
    );
    result.insert(
        TableKey::Keyword("height".into()),
        Value::float(layout.height),
    );

    (SIG_OK, Value::struct_from(result))
}

fn prim_fdg_render(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("fdg/render: expected 2 arguments, got {}", args.len()),
            ),
        );
    }
    let node_data = match extract_nodes(&args[0]) {
        Ok(n) => n,
        Err(e) => return (SIG_ERROR, error_val("type-error", e)),
    };
    let edge_data = match extract_edges(&args[1]) {
        Ok(e) => e,
        Err(e) => return (SIG_ERROR, error_val("type-error", e)),
    };
    let layout = match compute_layout(&node_data, &edge_data) {
        Ok(l) => l,
        Err(e) => return (SIG_ERROR, error_val("fdg-error", e)),
    };

    let svg = render_svg(&layout, &edge_data);
    (SIG_OK, Value::string(&*svg))
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "fdg/layout",
        func: prim_fdg_layout,
        effect: Effect::raises(),
        arity: Arity::Exact(2),
        doc: "Compute force-directed graph layout positions",
        params: &["nodes", "edges"],
        category: "fdg",
        example: r#"(fdg/layout '((0 "A") (1 "B")) '((0 1)))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "fdg/render",
        func: prim_fdg_render,
        effect: Effect::raises(),
        arity: Arity::Exact(2),
        doc: "Compute force-directed graph layout and render to SVG",
        params: &["nodes", "edges"],
        category: "fdg",
        example: r#"(fdg/render '((0 "A") (1 "B")) '((0 1)))"#,
        aliases: &[],
    },
];
