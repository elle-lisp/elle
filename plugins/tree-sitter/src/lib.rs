//! Elle tree-sitter plugin — multi-language parsing and structural queries.
//!
//! Provides a query-first API for parsing and inspecting syntax trees.
//! Bundled grammars: C, Rust. (Elle grammar: future work.)

use elle::list;
use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};
use std::collections::BTreeMap;
use std::rc::Rc;
use streaming_iterator::StreamingIterator;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, Tree};

// ---------------------------------------------------------------------------
// Internal data types (stored as Value::external)
// ---------------------------------------------------------------------------

/// Parsed tree + its source text. Shared via Rc so nodes can reference it.
struct TsTreeData {
    tree: Tree,
    source: String,
}

/// A node identified by its path from the root (vector of child indices).
/// Safe: no lifetime issues, no raw pointers. The Rc keeps the tree alive.
struct TsNodeData {
    tree_data: Rc<TsTreeData>,
    path: Vec<usize>,
}

/// A compiled tree-sitter query.
struct TsQueryData {
    query: Query,
}

impl TsNodeData {
    /// Reconstruct the tree-sitter Node by walking from root.
    fn resolve(&self) -> Option<Node<'_>> {
        let mut node = self.tree_data.tree.root_node();
        for &idx in &self.path {
            node = node.child(idx)?;
        }
        Some(node)
    }

    /// Build a TsNodeData from a live Node by computing its path from root.
    fn from_node(node: Node<'_>, tree_data: Rc<TsTreeData>) -> Self {
        TsNodeData {
            tree_data,
            path: compute_path(node),
        }
    }
}

/// Walk up from a node to root, collecting the child index at each level.
fn compute_path(node: Node<'_>) -> Vec<usize> {
    let mut path = Vec::new();
    let mut current = node;
    while let Some(parent) = current.parent() {
        // Find which child index `current` is within `parent`.
        let id = current.id();
        for i in 0..parent.child_count() {
            if let Some(child) = parent.child(i) {
                if child.id() == id {
                    path.push(i);
                    break;
                }
            }
        }
        current = parent;
    }
    path.reverse();
    path
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_string(args: &[Value], idx: usize, prim: &str) -> Result<String, (SignalBits, Value)> {
    args[idx].with_string(|s| s.to_string()).ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected string, got {}", prim, args[idx].type_name()),
            ),
        )
    })
}

fn get_tree<'a>(
    args: &'a [Value],
    idx: usize,
    prim: &str,
) -> Result<&'a Rc<TsTreeData>, (SignalBits, Value)> {
    args[idx].as_external::<Rc<TsTreeData>>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected ts/tree, got {}", prim, args[idx].type_name()),
            ),
        )
    })
}

fn get_tree_rc(
    args: &[Value],
    idx: usize,
    prim: &str,
) -> Result<Rc<TsTreeData>, (SignalBits, Value)> {
    args[idx]
        .as_external::<Rc<TsTreeData>>()
        .cloned()
        .ok_or_else(|| {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{}: expected ts/tree, got {}", prim, args[idx].type_name()),
                ),
            )
        })
}

fn get_node<'a>(
    args: &'a [Value],
    idx: usize,
    prim: &str,
) -> Result<&'a TsNodeData, (SignalBits, Value)> {
    args[idx].as_external::<TsNodeData>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected ts/node, got {}", prim, args[idx].type_name()),
            ),
        )
    })
}

fn get_query<'a>(
    args: &'a [Value],
    idx: usize,
    prim: &str,
) -> Result<&'a TsQueryData, (SignalBits, Value)> {
    args[idx].as_external::<TsQueryData>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected ts/query, got {}", prim, args[idx].type_name()),
            ),
        )
    })
}

fn get_language(args: &[Value], idx: usize, prim: &str) -> Result<Language, (SignalBits, Value)> {
    args[idx].as_external::<Language>().cloned().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected ts/language, got {}",
                    prim,
                    args[idx].type_name()
                ),
            ),
        )
    })
}

fn node_to_value(node: Node<'_>, tree_data: Rc<TsTreeData>) -> Value {
    Value::external("ts/node", TsNodeData::from_node(node, tree_data))
}

fn range_to_value(node: &Node<'_>) -> Value {
    let start = node.start_position();
    let end = node.end_position();
    let mut fields = BTreeMap::new();
    fields.insert(
        TableKey::Keyword("start-row".into()),
        Value::int(start.row as i64),
    );
    fields.insert(
        TableKey::Keyword("start-col".into()),
        Value::int(start.column as i64),
    );
    fields.insert(
        TableKey::Keyword("end-row".into()),
        Value::int(end.row as i64),
    );
    fields.insert(
        TableKey::Keyword("end-col".into()),
        Value::int(end.column as i64),
    );
    fields.insert(
        TableKey::Keyword("start-byte".into()),
        Value::int(node.start_byte() as i64),
    );
    fields.insert(
        TableKey::Keyword("end-byte".into()),
        Value::int(node.end_byte() as i64),
    );
    Value::struct_from(fields)
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

/// (ts/language name-string) → language object
fn prim_ts_language(args: &[Value]) -> (SignalBits, Value) {
    let name = match get_string(args, 0, "ts/language") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let lang: Language = match name.as_str() {
        "c" => tree_sitter_c::LANGUAGE.into(),
        "rust" => tree_sitter_rust::LANGUAGE.into(),
        other => {
            return (
                SIG_ERROR,
                error_val(
                    "value-error",
                    format!("ts/language: unknown language {:?}", other),
                ),
            )
        }
    };
    (SIG_OK, Value::external("ts/language", lang))
}

/// (ts/parse source-string language) → tree
fn prim_ts_parse(args: &[Value]) -> (SignalBits, Value) {
    let source = match get_string(args, 0, "ts/parse") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let lang = match get_language(args, 1, "ts/parse") {
        Ok(l) => l,
        Err(e) => return e,
    };
    let mut parser = Parser::new();
    if let Err(e) = parser.set_language(&lang) {
        return (
            SIG_ERROR,
            error_val("parse-error", format!("ts/parse: {}", e)),
        );
    }
    match parser.parse(&source, None) {
        Some(tree) => {
            let data = Rc::new(TsTreeData { tree, source });
            (SIG_OK, Value::external("ts/tree", data))
        }
        None => (
            SIG_ERROR,
            error_val("parse-error", "ts/parse: parsing failed"),
        ),
    }
}

/// (ts/root tree) → node
fn prim_ts_root(args: &[Value]) -> (SignalBits, Value) {
    let tree_rc = match get_tree_rc(args, 0, "ts/root") {
        Ok(t) => t,
        Err(e) => return e,
    };
    // Must get root node from the Rc'd tree (not a local), so the borrow
    // is against data that outlives this frame via the Rc.
    let root = tree_rc.tree.root_node();
    let path = compute_path(root);
    let nd = TsNodeData {
        tree_data: tree_rc,
        path,
    };
    (SIG_OK, Value::external("ts/node", nd))
}

/// (ts/node-type node) → string
fn prim_ts_node_type(args: &[Value]) -> (SignalBits, Value) {
    let nd = match get_node(args, 0, "ts/node-type") {
        Ok(n) => n,
        Err(e) => return e,
    };
    match nd.resolve() {
        Some(node) => (SIG_OK, Value::string(node.kind())),
        None => (
            SIG_ERROR,
            error_val("node-error", "ts/node-type: could not resolve node"),
        ),
    }
}

/// (ts/node-text node) → string
fn prim_ts_node_text(args: &[Value]) -> (SignalBits, Value) {
    let nd = match get_node(args, 0, "ts/node-text") {
        Ok(n) => n,
        Err(e) => return e,
    };
    match nd.resolve() {
        Some(node) => {
            let text = &nd.tree_data.source[node.start_byte()..node.end_byte()];
            (SIG_OK, Value::string(text))
        }
        None => (
            SIG_ERROR,
            error_val("node-error", "ts/node-text: could not resolve node"),
        ),
    }
}

/// (ts/node-named? node) → bool
fn prim_ts_node_named(args: &[Value]) -> (SignalBits, Value) {
    let nd = match get_node(args, 0, "ts/node-named?") {
        Ok(n) => n,
        Err(e) => return e,
    };
    match nd.resolve() {
        Some(node) => (SIG_OK, Value::bool(node.is_named())),
        None => (
            SIG_ERROR,
            error_val("node-error", "ts/node-named?: could not resolve node"),
        ),
    }
}

/// (ts/children node) → list of nodes
fn prim_ts_children(args: &[Value]) -> (SignalBits, Value) {
    let nd = match get_node(args, 0, "ts/children") {
        Ok(n) => n,
        Err(e) => return e,
    };
    match nd.resolve() {
        Some(node) => {
            let tree_data = nd.tree_data.clone();
            let children: Vec<Value> = (0..node.child_count())
                .filter_map(|i| node.child(i))
                .map(|child| node_to_value(child, tree_data.clone()))
                .collect();
            (SIG_OK, list(children))
        }
        None => (
            SIG_ERROR,
            error_val("node-error", "ts/children: could not resolve node"),
        ),
    }
}

/// (ts/named-children node) → list of named child nodes
fn prim_ts_named_children(args: &[Value]) -> (SignalBits, Value) {
    let nd = match get_node(args, 0, "ts/named-children") {
        Ok(n) => n,
        Err(e) => return e,
    };
    match nd.resolve() {
        Some(node) => {
            let tree_data = nd.tree_data.clone();
            let children: Vec<Value> = (0..node.named_child_count())
                .filter_map(|i| node.named_child(i))
                .map(|child| node_to_value(child, tree_data.clone()))
                .collect();
            (SIG_OK, list(children))
        }
        None => (
            SIG_ERROR,
            error_val("node-error", "ts/named-children: could not resolve node"),
        ),
    }
}

/// (ts/child-by-field node field-name) → node or nil
fn prim_ts_child_by_field(args: &[Value]) -> (SignalBits, Value) {
    let nd = match get_node(args, 0, "ts/child-by-field") {
        Ok(n) => n,
        Err(e) => return e,
    };
    let field = match get_string(args, 1, "ts/child-by-field") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match nd.resolve() {
        Some(node) => match node.child_by_field_name(&field) {
            Some(child) => (SIG_OK, node_to_value(child, nd.tree_data.clone())),
            None => (SIG_OK, Value::NIL),
        },
        None => (
            SIG_ERROR,
            error_val("node-error", "ts/child-by-field: could not resolve node"),
        ),
    }
}

/// (ts/parent node) → node or nil
fn prim_ts_parent(args: &[Value]) -> (SignalBits, Value) {
    let nd = match get_node(args, 0, "ts/parent") {
        Ok(n) => n,
        Err(e) => return e,
    };
    if nd.path.is_empty() {
        // Root node has no parent
        return (SIG_OK, Value::NIL);
    }
    // Parent path is just our path without the last element
    let parent = TsNodeData {
        tree_data: nd.tree_data.clone(),
        path: nd.path[..nd.path.len() - 1].to_vec(),
    };
    (SIG_OK, Value::external("ts/node", parent))
}

/// (ts/node-range node) → {:start-row :start-col :end-row :end-col :start-byte :end-byte}
fn prim_ts_node_range(args: &[Value]) -> (SignalBits, Value) {
    let nd = match get_node(args, 0, "ts/node-range") {
        Ok(n) => n,
        Err(e) => return e,
    };
    match nd.resolve() {
        Some(node) => (SIG_OK, range_to_value(&node)),
        None => (
            SIG_ERROR,
            error_val("node-error", "ts/node-range: could not resolve node"),
        ),
    }
}

/// (ts/node-sexp node) → string (S-expression debug format)
fn prim_ts_node_sexp(args: &[Value]) -> (SignalBits, Value) {
    let nd = match get_node(args, 0, "ts/node-sexp") {
        Ok(n) => n,
        Err(e) => return e,
    };
    match nd.resolve() {
        Some(node) => {
            let sexp = node.to_sexp();
            (SIG_OK, Value::string(&*sexp))
        }
        None => (
            SIG_ERROR,
            error_val("node-error", "ts/node-sexp: could not resolve node"),
        ),
    }
}

/// (ts/query language pattern-string) → compiled query
fn prim_ts_query(args: &[Value]) -> (SignalBits, Value) {
    let lang = match get_language(args, 0, "ts/query") {
        Ok(l) => l,
        Err(e) => return e,
    };
    let pattern = match get_string(args, 1, "ts/query") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match Query::new(&lang, &pattern) {
        Ok(query) => (SIG_OK, Value::external("ts/query", TsQueryData { query })),
        Err(e) => (
            SIG_ERROR,
            error_val("query-error", format!("ts/query: {}", e)),
        ),
    }
}

/// (ts/matches query node) → list of {:pattern index :captures {:name node ...}}
fn prim_ts_matches(args: &[Value]) -> (SignalBits, Value) {
    let qd = match get_query(args, 0, "ts/matches") {
        Ok(q) => q,
        Err(e) => return e,
    };
    let nd = match get_node(args, 1, "ts/matches") {
        Ok(n) => n,
        Err(e) => return e,
    };
    let node = match nd.resolve() {
        Some(n) => n,
        None => {
            return (
                SIG_ERROR,
                error_val("node-error", "ts/matches: could not resolve node"),
            )
        }
    };

    let capture_names = qd.query.capture_names();
    let tree_data = nd.tree_data.clone();
    let source = tree_data.source.as_bytes();

    let mut cursor = QueryCursor::new();
    let mut results: Vec<Value> = Vec::new();
    let mut iter = cursor.matches(&qd.query, node, source);
    while let Some(m) = iter.next() {
        let mut captures = BTreeMap::new();
        for cap in m.captures {
            let name = &capture_names[cap.index as usize];
            captures.insert(
                TableKey::Keyword((*name).into()),
                node_to_value(cap.node, tree_data.clone()),
            );
        }
        let mut fields = BTreeMap::new();
        fields.insert(
            TableKey::Keyword("pattern".into()),
            Value::int(m.pattern_index as i64),
        );
        fields.insert(
            TableKey::Keyword("captures".into()),
            Value::struct_from(captures),
        );
        results.push(Value::struct_from(fields));
    }

    (SIG_OK, list(results))
}

/// (ts/captures query node) → list of {:name string :node node}
fn prim_ts_captures(args: &[Value]) -> (SignalBits, Value) {
    let qd = match get_query(args, 0, "ts/captures") {
        Ok(q) => q,
        Err(e) => return e,
    };
    let nd = match get_node(args, 1, "ts/captures") {
        Ok(n) => n,
        Err(e) => return e,
    };
    let node = match nd.resolve() {
        Some(n) => n,
        None => {
            return (
                SIG_ERROR,
                error_val("node-error", "ts/captures: could not resolve node"),
            )
        }
    };

    let capture_names = qd.query.capture_names();
    let tree_data = nd.tree_data.clone();
    let source = tree_data.source.as_bytes();

    let mut cursor = QueryCursor::new();
    let mut results: Vec<Value> = Vec::new();
    let mut iter = cursor.captures(&qd.query, node, source);
    while let Some((m, _capture_idx)) = iter.next() {
        for cap in m.captures {
            let name = &capture_names[cap.index as usize];
            let mut fields = BTreeMap::new();
            fields.insert(TableKey::Keyword("name".into()), Value::string(*name));
            fields.insert(
                TableKey::Keyword("node".into()),
                node_to_value(cap.node, tree_data.clone()),
            );
            results.push(Value::struct_from(fields));
        }
    }

    (SIG_OK, list(results))
}

/// (ts/node-count tree) → int (total nodes in tree — useful for benchmarks)
fn prim_ts_node_count(args: &[Value]) -> (SignalBits, Value) {
    let tree = match get_tree(args, 0, "ts/node-count") {
        Ok(t) => t,
        Err(e) => return e,
    };
    fn count(node: Node<'_>) -> i64 {
        let mut n: i64 = 1;
        for i in 0..node.child_count() {
            if let Some(child) = node.child(i) {
                n += count(child);
            }
        }
        n
    }
    (SIG_OK, Value::int(count(tree.tree.root_node())))
}

// ---------------------------------------------------------------------------
// Plugin entry point
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "ts/language",
        func: prim_ts_language,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Load a built-in tree-sitter grammar by name. Supported: \"c\", \"rust\"",
        params: &["name"],
        category: "tree-sitter",
        example: r#"(ts/language "c")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "ts/parse",
        func: prim_ts_parse,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Parse source code with a language grammar, returning a tree",
        params: &["source", "language"],
        category: "tree-sitter",
        example: r#"(ts/parse "int main() {}" (ts/language "c"))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "ts/root",
        func: prim_ts_root,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Get the root node of a parsed tree",
        params: &["tree"],
        category: "tree-sitter",
        example: r#"(ts/root tree)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "ts/node-type",
        func: prim_ts_node_type,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the grammar node type as a string (e.g. \"function_definition\")",
        params: &["node"],
        category: "tree-sitter",
        example: r#"(ts/node-type node)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "ts/node-text",
        func: prim_ts_node_text,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the source text spanned by a node",
        params: &["node"],
        category: "tree-sitter",
        example: r#"(ts/node-text node)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "ts/node-named?",
        func: prim_ts_node_named,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return true if the node is a named node (not anonymous punctuation/keywords)",
        params: &["node"],
        category: "tree-sitter",
        example: r#"(ts/node-named? node)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "ts/children",
        func: prim_ts_children,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return all child nodes (including anonymous) as a list",
        params: &["node"],
        category: "tree-sitter",
        example: r#"(ts/children node)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "ts/named-children",
        func: prim_ts_named_children,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return only named child nodes (skip punctuation/keywords) as a list",
        params: &["node"],
        category: "tree-sitter",
        example: r#"(ts/named-children node)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "ts/child-by-field",
        func: prim_ts_child_by_field,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Get a child node by its field name (grammar-defined), or nil if absent",
        params: &["node", "field-name"],
        category: "tree-sitter",
        example: r#"(ts/child-by-field node "name")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "ts/parent",
        func: prim_ts_parent,
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Return the parent node, or nil for the root",
        params: &["node"],
        category: "tree-sitter",
        example: r#"(ts/parent node)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "ts/node-range",
        func: prim_ts_node_range,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return {:start-row :start-col :end-row :end-col :start-byte :end-byte} for a node",
        params: &["node"],
        category: "tree-sitter",
        example: r#"(ts/node-range node)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "ts/node-sexp",
        func: prim_ts_node_sexp,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the S-expression representation of a node (tree-sitter debug format)",
        params: &["node"],
        category: "tree-sitter",
        example: r#"(ts/node-sexp node)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "ts/query",
        func: prim_ts_query,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Compile a tree-sitter query pattern for a language",
        params: &["language", "pattern"],
        category: "tree-sitter",
        example: r#"(ts/query lang "(function_definition name: (identifier) @name)")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "ts/matches",
        func: prim_ts_matches,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Run a query against a node, returning a list of {:pattern int :captures {:name node ...}}",
        params: &["query", "node"],
        category: "tree-sitter",
        example: r#"(ts/matches query (ts/root tree))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "ts/captures",
        func: prim_ts_captures,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Run a query, returning a flat list of {:name string :node node}",
        params: &["query", "node"],
        category: "tree-sitter",
        example: r#"(ts/captures query (ts/root tree))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "ts/node-count",
        func: prim_ts_node_count,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the total number of nodes in a parsed tree",
        params: &["tree"],
        category: "tree-sitter",
        example: r#"(ts/node-count tree)"#,
        aliases: &[],
    },
];

#[no_mangle]
/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`. The caller must pass a valid
/// `PluginContext` reference. Only safe when called from `load_plugin`.
pub unsafe extern "C" fn elle_plugin_init(ctx: &mut PluginContext) -> Value {
    ctx.init_keywords();

    let mut fields = BTreeMap::new();
    for def in PRIMITIVES {
        ctx.register(def);
        let short_name = def.name.strip_prefix("ts/").unwrap_or(def.name);
        fields.insert(
            TableKey::Keyword(short_name.into()),
            Value::native_fn(def.func),
        );
    }
    Value::struct_from(fields)
}
