//! Elle oxigraph plugin — RDF quad storage + SPARQL via the `oxigraph` crate.

use std::collections::BTreeMap;

use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};

use oxigraph::model::{BlankNode, GraphName, Literal, NamedNode, Term};
use oxigraph::store::Store;

// ---------------------------------------------------------------------------
// Plugin entry point
// ---------------------------------------------------------------------------

/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`. The caller must pass a valid
/// `PluginContext` reference. Only safe when called from `load_plugin`.
#[no_mangle]
pub unsafe extern "C" fn elle_plugin_init(ctx: &mut PluginContext) -> Value {
    let mut fields = BTreeMap::new();
    for def in PRIMITIVES {
        ctx.register(def);
        let short_name = def.name.strip_prefix("oxigraph/").unwrap_or(def.name);
        fields.insert(
            TableKey::Keyword(short_name.into()),
            Value::native_fn(def.func),
        );
    }
    Value::struct_from(fields)
}

// ---------------------------------------------------------------------------
// Term conversion helpers
// ---------------------------------------------------------------------------

/// Build an Elle array representing an RDF IRI: `[:iri "http://..."]`.
fn iri_to_elle(n: &NamedNode) -> Value {
    Value::array(vec![Value::keyword("iri"), Value::string(n.as_str())])
}

/// Build an Elle array representing an RDF blank node: `[:bnode "id"]`.
fn bnode_to_elle(b: &BlankNode) -> Value {
    Value::array(vec![Value::keyword("bnode"), Value::string(b.as_str())])
}

#[allow(dead_code)]
/// Build an Elle array representing an RDF literal.
///
/// Plain:    `[:literal "hello"]`
/// Language: `[:literal "hello" :lang "en"]`
/// Datatype: `[:literal "hello" :datatype "http://..."]`
fn literal_to_elle(l: &Literal) -> Value {
    if let Some(lang) = l.language() {
        Value::array(vec![
            Value::keyword("literal"),
            Value::string(l.value()),
            Value::keyword("lang"),
            Value::string(lang),
        ])
    } else {
        let dt = l.datatype().as_str();
        // xsd:string is the implicit datatype for plain literals — omit it.
        const XSD_STRING: &str = "http://www.w3.org/2001/XMLSchema#string";
        if dt == XSD_STRING {
            Value::array(vec![Value::keyword("literal"), Value::string(l.value())])
        } else {
            Value::array(vec![
                Value::keyword("literal"),
                Value::string(l.value()),
                Value::keyword("datatype"),
                Value::string(dt),
            ])
        }
    }
}

#[allow(dead_code)]
/// Convert an oxigraph `Term` to an Elle array.
fn term_to_elle(term: &Term) -> Value {
    match term {
        Term::NamedNode(n) => iri_to_elle(n),
        Term::BlankNode(b) => bnode_to_elle(b),
        Term::Literal(l) => literal_to_elle(l),
        Term::Triple(_) => Value::NIL, // rdf-star triple terms not supported
    }
}

#[allow(dead_code)]
/// Convert an Elle term array to an oxigraph `Term`.
///
/// Returns an error `(SignalBits, Value)` if the shape is invalid.
fn elle_to_term(val: Value, prim: &str) -> Result<Term, (SignalBits, Value)> {
    let elems = val.as_array().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected term array, got {}", prim, val.type_name()),
            ),
        )
    })?;

    let tag = elems
        .first()
        .and_then(|v| v.as_keyword_name())
        .ok_or_else(|| {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{}: term array must start with a keyword tag", prim),
                ),
            )
        })?;

    match tag {
        "iri" => {
            let s = string_at(elems, 1, prim, "IRI string")?;
            NamedNode::new(s.clone())
                .map(Term::from)
                .map_err(|e| oxigraph_err(prim, e))
        }
        "bnode" => {
            let s = string_at(elems, 1, prim, "blank node id")?;
            BlankNode::new(s.clone())
                .map(Term::from)
                .map_err(|e| oxigraph_err(prim, e))
        }
        "literal" => {
            let value = string_at(elems, 1, prim, "literal value")?;
            if elems.len() == 2 {
                Ok(Term::from(Literal::new_simple_literal(value.clone())))
            } else if elems.len() == 4 {
                let key = elems[2].as_keyword_name().ok_or_else(|| {
                    (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!("{}: expected :lang or :datatype keyword at index 2", prim),
                        ),
                    )
                })?;
                let tag_val = string_at(elems, 3, prim, "tag value")?;
                match key {
                    "lang" => Literal::new_language_tagged_literal(value.clone(), tag_val.clone())
                        .map(Term::from)
                        .map_err(|e| oxigraph_err(prim, e)),
                    "datatype" => {
                        let dt =
                            NamedNode::new(tag_val.clone()).map_err(|e| oxigraph_err(prim, e))?;
                        Ok(Term::from(Literal::new_typed_literal(value.clone(), dt)))
                    }
                    _ => Err((
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!("{}: expected :lang or :datatype, got :{}", prim, key),
                        ),
                    )),
                }
            } else {
                Err((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "{}: :literal array must have length 2 or 4, got {}",
                            prim,
                            elems.len()
                        ),
                    ),
                ))
            }
        }
        _ => Err((
            SIG_ERROR,
            error_val("type-error", format!("{}: unknown term tag :{}", prim, tag)),
        )),
    }
}

#[allow(dead_code)]
/// Convert an Elle graph-name value to an oxigraph `GraphName`.
///
/// `nil` → `DefaultGraph`, `[:iri "..."]` or `[:bnode "..."]` → named/blank.
fn elle_to_graph_name(val: Value, prim: &str) -> Result<GraphName, (SignalBits, Value)> {
    if val.is_nil() {
        return Ok(GraphName::DefaultGraph);
    }
    let term = elle_to_term(val, prim)?;
    match term {
        Term::NamedNode(n) => Ok(GraphName::NamedNode(n)),
        Term::BlankNode(b) => Ok(GraphName::BlankNode(b)),
        Term::Literal(_) | Term::Triple(_) => Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: graph name must be an IRI or blank node", prim),
            ),
        )),
    }
}

// ---------------------------------------------------------------------------
// Small helpers
// ---------------------------------------------------------------------------

/// Extract a string from `elems[index]`, owned.
fn string_at(
    elems: &[Value],
    index: usize,
    prim: &str,
    what: &str,
) -> Result<String, (SignalBits, Value)> {
    elems
        .get(index)
        .and_then(|v| v.with_string(|s| s.to_string()))
        .ok_or_else(|| {
            (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: expected string at index {} ({}), got {}",
                        prim,
                        index,
                        what,
                        elems.get(index).map_or("(missing)", |v| v.type_name())
                    ),
                ),
            )
        })
}

/// Map any `Display` error to an `oxigraph-error` signal.
fn oxigraph_err(prim: &str, e: impl std::fmt::Display) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val("oxigraph-error", format!("{}: {}", prim, e)),
    )
}

#[allow(dead_code)]
/// Extract `Store` from `args[0]`, or return a type-error.
fn get_store<'a>(args: &'a [Value], prim: &str) -> Result<&'a Store, (SignalBits, Value)> {
    args[0].as_external::<Store>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected oxigraph/store, got {}",
                    prim,
                    args[0].type_name()
                ),
            ),
        )
    })
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_store_new(_args: &[Value]) -> (SignalBits, Value) {
    match Store::new() {
        Ok(store) => (SIG_OK, Value::external("oxigraph/store", store)),
        Err(e) => oxigraph_err("oxigraph/store-new", e),
    }
}

fn prim_store_open(args: &[Value]) -> (SignalBits, Value) {
    let path = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "oxigraph/store-open: expected string path, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    match Store::open(&path) {
        Ok(store) => (SIG_OK, Value::external("oxigraph/store", store)),
        Err(e) => oxigraph_err("oxigraph/store-open", e),
    }
}

fn prim_iri(args: &[Value]) -> (SignalBits, Value) {
    let s = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("oxigraph/iri: expected string, got {}", args[0].type_name()),
                ),
            );
        }
    };
    match NamedNode::new(s) {
        Ok(n) => (SIG_OK, iri_to_elle(&n)),
        Err(e) => oxigraph_err("oxigraph/iri", e),
    }
}

fn prim_literal(args: &[Value]) -> (SignalBits, Value) {
    let value = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "oxigraph/literal: expected string value, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };

    if args.len() == 1 {
        // Plain literal.
        return (
            SIG_OK,
            Value::array(vec![Value::keyword("literal"), Value::string(value)]),
        );
    }

    // 3-argument form: value, tag-key, tag-value.
    let tag_key = match args[1].as_keyword_name() {
        Some(k) => k.to_string(),
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "oxigraph/literal: expected :lang or :datatype keyword, got {}",
                        args[1].type_name()
                    ),
                ),
            );
        }
    };
    let tag_val = match args[2].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "oxigraph/literal: expected string tag value, got {}",
                        args[2].type_name()
                    ),
                ),
            );
        }
    };

    match tag_key.as_str() {
        "lang" => {
            // Validate language tag via Literal constructor.
            match Literal::new_language_tagged_literal(&value, &tag_val) {
                Ok(_) => (
                    SIG_OK,
                    Value::array(vec![
                        Value::keyword("literal"),
                        Value::string(value),
                        Value::keyword("lang"),
                        Value::string(tag_val),
                    ]),
                ),
                Err(e) => oxigraph_err("oxigraph/literal", e),
            }
        }
        "datatype" => {
            // Store datatype IRI as-is; validate at insert time.
            (
                SIG_OK,
                Value::array(vec![
                    Value::keyword("literal"),
                    Value::string(value),
                    Value::keyword("datatype"),
                    Value::string(tag_val),
                ]),
            )
        }
        _ => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "oxigraph/literal: expected :lang or :datatype, got :{}",
                    tag_key
                ),
            ),
        ),
    }
}

fn prim_blank_node(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        let b = BlankNode::default();
        return (SIG_OK, bnode_to_elle(&b));
    }
    let id = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "oxigraph/blank-node: expected string id, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    match BlankNode::new(id) {
        Ok(b) => (SIG_OK, bnode_to_elle(&b)),
        Err(e) => oxigraph_err("oxigraph/blank-node", e),
    }
}

// ---------------------------------------------------------------------------
// Stub primitives (Chunks 2–4 — not-yet-implemented)
// ---------------------------------------------------------------------------

fn prim_insert(_args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val("not-implemented", "oxigraph/insert: not yet implemented"),
    )
}

fn prim_remove(_args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val("not-implemented", "oxigraph/remove: not yet implemented"),
    )
}

fn prim_contains(_args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val("not-implemented", "oxigraph/contains: not yet implemented"),
    )
}

fn prim_quads(_args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val("not-implemented", "oxigraph/quads: not yet implemented"),
    )
}

fn prim_query(_args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val("not-implemented", "oxigraph/query: not yet implemented"),
    )
}

fn prim_update(_args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val("not-implemented", "oxigraph/update: not yet implemented"),
    )
}

fn prim_load(_args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val("not-implemented", "oxigraph/load: not yet implemented"),
    )
}

fn prim_dump(_args: &[Value]) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val("not-implemented", "oxigraph/dump: not yet implemented"),
    )
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "oxigraph/store-new",
        func: prim_store_new,
        signal: Signal::errors(),
        arity: Arity::Exact(0),
        doc: "Create a new in-memory RDF store.",
        params: &[],
        category: "oxigraph",
        example: "(oxigraph/store-new)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "oxigraph/store-open",
        func: prim_store_open,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Open a persistent on-disk RDF store at the given path.",
        params: &["path"],
        category: "oxigraph",
        example: r#"(oxigraph/store-open "/tmp/my-graph")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "oxigraph/iri",
        func: prim_iri,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Construct and validate an IRI term. Returns [:iri \"http://...\"].",
        params: &["iri-string"],
        category: "oxigraph",
        example: r#"(oxigraph/iri "http://example.org/alice")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "oxigraph/literal",
        func: prim_literal,
        signal: Signal::errors(),
        arity: Arity::Range(1, 3),
        doc: "Construct a literal term. 1 arg = plain. 3 args = :lang or :datatype tagged.",
        params: &["value", "tag-key?", "tag-value?"],
        category: "oxigraph",
        example: r#"(oxigraph/literal "hello" :lang "en")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "oxigraph/blank-node",
        func: prim_blank_node,
        signal: Signal::errors(),
        arity: Arity::Range(0, 1),
        doc: "Construct a blank node. 0 args = auto-generated ID. 1 arg = explicit ID.",
        params: &["id?"],
        category: "oxigraph",
        example: "(oxigraph/blank-node)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "oxigraph/insert",
        func: prim_insert,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Insert a quad into the store. (not yet implemented)",
        params: &["store", "quad"],
        category: "oxigraph",
        example: "(oxigraph/insert store [s p o nil])",
        aliases: &[],
    },
    PrimitiveDef {
        name: "oxigraph/remove",
        func: prim_remove,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Remove a quad from the store. (not yet implemented)",
        params: &["store", "quad"],
        category: "oxigraph",
        example: "(oxigraph/remove store quad)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "oxigraph/contains",
        func: prim_contains,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Check if a quad exists in the store. (not yet implemented)",
        params: &["store", "quad"],
        category: "oxigraph",
        example: "(oxigraph/contains store quad)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "oxigraph/quads",
        func: prim_quads,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return all quads in the store as an array. (not yet implemented)",
        params: &["store"],
        category: "oxigraph",
        example: "(oxigraph/quads store)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "oxigraph/query",
        func: prim_query,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Execute a SPARQL query against the store. (not yet implemented)",
        params: &["store", "sparql"],
        category: "oxigraph",
        example: r#"(oxigraph/query store "SELECT ?s ?p ?o WHERE { ?s ?p ?o }")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "oxigraph/update",
        func: prim_update,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Execute a SPARQL UPDATE against the store. (not yet implemented)",
        params: &["store", "sparql-update"],
        category: "oxigraph",
        example: r#"(oxigraph/update store "INSERT DATA { ... }")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "oxigraph/load",
        func: prim_load,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Load RDF data from a string into the store. (not yet implemented)",
        params: &["store", "data", "format"],
        category: "oxigraph",
        example: r#"(oxigraph/load store "<http://ex.org/a> <http://ex.org/b> \"hello\" .\n" :ntriples)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "oxigraph/dump",
        func: prim_dump,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Serialize all store data to a string. (not yet implemented)",
        params: &["store", "format"],
        category: "oxigraph",
        example: "(oxigraph/dump store :nquads)",
        aliases: &[],
    },
];
