//! Elle oxigraph plugin — RDF quad storage + SPARQL via the `oxigraph` crate.

use std::collections::BTreeMap;

use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};

use oxigraph::io::{RdfFormat, RdfSerializer};
use oxigraph::model::{
    BlankNode, GraphName, GraphNameRef, Literal, NamedNode, Quad, Subject, Term,
};
use oxigraph::sparql::QueryResults;
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

/// Convert an oxigraph `Term` to an Elle array.
fn term_to_elle(term: &Term) -> Value {
    match term {
        Term::NamedNode(n) => iri_to_elle(n),
        Term::BlankNode(b) => bnode_to_elle(b),
        Term::Literal(l) => literal_to_elle(l),
        Term::Triple(_) => Value::NIL, // rdf-star triple terms not supported
    }
}

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

    match tag.as_str() {
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
                match key.as_str() {
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
// Quad conversion helpers
// ---------------------------------------------------------------------------

/// Convert an oxigraph `GraphName` to an Elle value.
///
/// `DefaultGraph` → `nil`, named/blank → term array.
fn graph_name_to_elle(gn: &GraphName) -> Value {
    match gn {
        GraphName::DefaultGraph => Value::NIL,
        GraphName::NamedNode(n) => iri_to_elle(n),
        GraphName::BlankNode(b) => bnode_to_elle(b),
    }
}

/// Convert an oxigraph `Subject` to an Elle value.
fn subject_to_elle(s: &Subject) -> Value {
    match s {
        Subject::NamedNode(n) => iri_to_elle(n),
        Subject::BlankNode(b) => bnode_to_elle(b),
        Subject::Triple(_) => Value::NIL, // rdf-star not supported
    }
}

/// Convert an oxigraph `Quad` to a 4-element Elle array `[s p o g]`.
fn oxigraph_quad_to_elle(quad: &Quad) -> Value {
    Value::array(vec![
        subject_to_elle(&quad.subject),
        iri_to_elle(&quad.predicate),
        term_to_elle(&quad.object),
        graph_name_to_elle(&quad.graph_name),
    ])
}

/// Convert an Elle quad array `[s p o g]` to an oxigraph `Quad`.
///
/// Subject must be IRI or blank node. Predicate must be IRI.
/// Object can be any term. Graph-name is `nil` or IRI/blank node.
fn elle_quad_to_oxigraph(val: Value, prim: &str) -> Result<Quad, (SignalBits, Value)> {
    let elems = val.as_array().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected quad array, got {}", prim, val.type_name()),
            ),
        )
    })?;

    if elems.len() != 4 {
        return Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: quad array must have length 4, got {}",
                    prim,
                    elems.len()
                ),
            ),
        ));
    }

    // Subject: IRI or blank node only.
    let subject_term = elle_to_term(elems[0], prim)?;
    let subject: Subject = match subject_term {
        Term::NamedNode(n) => Subject::NamedNode(n),
        Term::BlankNode(b) => Subject::BlankNode(b),
        Term::Literal(_) | Term::Triple(_) => {
            return Err((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{}: subject must be an IRI or blank node", prim),
                ),
            ));
        }
    };

    // Predicate: IRI only.
    let pred_term = elle_to_term(elems[1], prim)?;
    let predicate = match pred_term {
        Term::NamedNode(n) => n,
        _ => {
            return Err((
                SIG_ERROR,
                error_val("type-error", format!("{}: predicate must be an IRI", prim)),
            ));
        }
    };

    // Object: any term.
    let object = elle_to_term(elems[2], prim)?;

    // Graph-name: nil, IRI, or blank node.
    let graph_name = elle_to_graph_name(elems[3], prim)?;

    Ok(Quad::new(subject, predicate, object, graph_name))
}

// ---------------------------------------------------------------------------
// Quad CRUD primitives
// ---------------------------------------------------------------------------

fn prim_insert(args: &[Value]) -> (SignalBits, Value) {
    let store = match get_store(args, "oxigraph/insert") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let quad = match elle_quad_to_oxigraph(args[1], "oxigraph/insert") {
        Ok(q) => q,
        Err(e) => return e,
    };
    match store.insert(quad.as_ref()) {
        Ok(_) => (SIG_OK, Value::NIL),
        Err(e) => oxigraph_err("oxigraph/insert", e),
    }
}

fn prim_remove(args: &[Value]) -> (SignalBits, Value) {
    let store = match get_store(args, "oxigraph/remove") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let quad = match elle_quad_to_oxigraph(args[1], "oxigraph/remove") {
        Ok(q) => q,
        Err(e) => return e,
    };
    match store.remove(quad.as_ref()) {
        Ok(_) => (SIG_OK, Value::NIL),
        Err(e) => oxigraph_err("oxigraph/remove", e),
    }
}

fn prim_contains(args: &[Value]) -> (SignalBits, Value) {
    let store = match get_store(args, "oxigraph/contains") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let quad = match elle_quad_to_oxigraph(args[1], "oxigraph/contains") {
        Ok(q) => q,
        Err(e) => return e,
    };
    match store.contains(quad.as_ref()) {
        Ok(result) => (SIG_OK, Value::bool(result)),
        Err(e) => oxigraph_err("oxigraph/contains", e),
    }
}

fn prim_quads(args: &[Value]) -> (SignalBits, Value) {
    let store = match get_store(args, "oxigraph/quads") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut result = Vec::new();
    for item in store.quads_for_pattern(None, None, None, None) {
        match item {
            Ok(quad) => result.push(oxigraph_quad_to_elle(&quad)),
            Err(e) => return oxigraph_err("oxigraph/quads", e),
        }
    }
    (SIG_OK, Value::array(result))
}

fn prim_query(args: &[Value]) -> (SignalBits, Value) {
    const PRIM: &str = "oxigraph/query";
    let store = match get_store(args, PRIM) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let sparql = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: expected string sparql, got {}",
                        PRIM,
                        args[1].type_name()
                    ),
                ),
            );
        }
    };
    let results = match store.query(sparql.as_str()) {
        Ok(r) => r,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("sparql-error", format!("{}: {}", PRIM, e)),
            )
        }
    };
    match results {
        QueryResults::Solutions(solutions) => {
            let mut rows: Vec<Value> = Vec::new();
            for solution in solutions {
                let solution = match solution {
                    Ok(s) => s,
                    Err(e) => {
                        return (
                            SIG_ERROR,
                            error_val("sparql-error", format!("{}: {}", PRIM, e)),
                        )
                    }
                };
                let mut fields = BTreeMap::new();
                for (variable, term) in solution.iter() {
                    fields.insert(
                        TableKey::Keyword(variable.as_str().into()),
                        term_to_elle(term),
                    );
                }
                rows.push(Value::struct_from(fields));
            }
            (SIG_OK, Value::array(rows))
        }
        QueryResults::Boolean(b) => (SIG_OK, Value::bool(b)),
        QueryResults::Graph(triples) => {
            let mut rows: Vec<Value> = Vec::new();
            for triple in triples {
                let triple = match triple {
                    Ok(t) => t,
                    Err(e) => {
                        return (
                            SIG_ERROR,
                            error_val("sparql-error", format!("{}: {}", PRIM, e)),
                        )
                    }
                };
                let subject_val = match &triple.subject {
                    Subject::NamedNode(n) => iri_to_elle(n),
                    Subject::BlankNode(b) => bnode_to_elle(b),
                    Subject::Triple(_) => Value::NIL,
                };
                rows.push(Value::array(vec![
                    subject_val,
                    iri_to_elle(&triple.predicate),
                    term_to_elle(&triple.object),
                    Value::NIL, // CONSTRUCT produces triples; graph-name is nil
                ]));
            }
            (SIG_OK, Value::array(rows))
        }
    }
}

fn prim_update(args: &[Value]) -> (SignalBits, Value) {
    const PRIM: &str = "oxigraph/update";
    let store = match get_store(args, PRIM) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let sparql = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: expected string sparql-update, got {}",
                        PRIM,
                        args[1].type_name()
                    ),
                ),
            );
        }
    };
    match store.update(sparql.as_str()) {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(e) => (
            SIG_ERROR,
            error_val("sparql-error", format!("{}: {}", PRIM, e)),
        ),
    }
}

/// Map a keyword value to an `RdfFormat`, or return a type-error.
fn keyword_to_format(val: Value, prim: &str) -> Result<RdfFormat, (SignalBits, Value)> {
    let kw = val.as_keyword_name().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected format keyword (:turtle :ntriples :nquads :rdfxml), got {}",
                    prim,
                    val.type_name()
                ),
            ),
        )
    })?;
    match kw.as_str() {
        "turtle" => Ok(RdfFormat::Turtle),
        "ntriples" => Ok(RdfFormat::NTriples),
        "nquads" => Ok(RdfFormat::NQuads),
        "rdfxml" => Ok(RdfFormat::RdfXml),
        _ => Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: unknown format keyword :{}, expected :turtle :ntriples :nquads :rdfxml",
                    prim, kw
                ),
            ),
        )),
    }
}

fn prim_load(args: &[Value]) -> (SignalBits, Value) {
    const PRIM: &str = "oxigraph/load";
    let store = match get_store(args, PRIM) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let data = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: expected string data, got {}",
                        PRIM,
                        args[1].type_name()
                    ),
                ),
            );
        }
    };
    let format = match keyword_to_format(args[2], PRIM) {
        Ok(f) => f,
        Err(e) => return e,
    };
    match store.load_from_reader(format, data.as_bytes()) {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(e) => oxigraph_err(PRIM, e),
    }
}

fn prim_dump(args: &[Value]) -> (SignalBits, Value) {
    const PRIM: &str = "oxigraph/dump";
    let store = match get_store(args, PRIM) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let format = match keyword_to_format(args[1], PRIM) {
        Ok(f) => f,
        Err(e) => return e,
    };
    // Dataset formats (NQuads, TriG) dump all graphs via dump_to_writer.
    // Graph formats (NTriples, Turtle, RdfXml) can only serialize a single
    // graph — we serialize the default graph.
    let buf: Vec<u8> = if format.supports_datasets() {
        match store.dump_to_writer(RdfSerializer::from_format(format), Vec::new()) {
            Ok(b) => b,
            Err(e) => return oxigraph_err(PRIM, e),
        }
    } else {
        match store.dump_graph_to_writer(
            GraphNameRef::DefaultGraph,
            RdfSerializer::from_format(format),
            Vec::new(),
        ) {
            Ok(b) => b,
            Err(e) => return oxigraph_err(PRIM, e),
        }
    };
    match String::from_utf8(buf) {
        Ok(s) => (SIG_OK, Value::string(s)),
        Err(e) => oxigraph_err(PRIM, e),
    }
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
        doc: "Insert a quad into the store.",
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
        doc: "Remove a quad from the store. No error if quad doesn't exist.",
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
        doc: "Check if a quad exists in the store.",
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
        doc: "Return all quads in the store as an immutable array.",
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
        doc: "Execute a SPARQL query against the store.",
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
        doc: "Execute a SPARQL UPDATE against the store.",
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
        doc:
            "Load RDF data from a string into the store. Format: :turtle :ntriples :nquads :rdfxml.",
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
        doc: "Serialize store to a string. Dataset formats (:nquads) dump all graphs; graph formats dump the default graph.",
        params: &["store", "format"],
        category: "oxigraph",
        example: "(oxigraph/dump store :nquads)",
        aliases: &[],
    },
];
