//! Elle xml plugin — XML parsing and serialization via the `quick-xml` crate.

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};
use quick_xml::events::{BytesEnd, BytesStart, BytesText, Event};
use quick_xml::Reader;
use quick_xml::Writer;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Cursor;

/// Plugin entry point. Called by Elle when loading the `.so`.
#[no_mangle]
/// # Safety
///
/// Called by Elle's plugin loader via `dlsym`. The caller must pass a valid
/// `PluginContext` reference. Only safe when called from `load_plugin`.
elle::elle_plugin_init!(PRIMITIVES, "xml/");

// ---------------------------------------------------------------------------
// DOM parser helpers
// ---------------------------------------------------------------------------

/// Internal element node during parsing.
struct ParsedElement {
    tag: String,
    attrs: BTreeMap<TableKey, Value>,
    children: Vec<Value>,
}

fn attrs_from_start(e: &BytesStart) -> Result<BTreeMap<TableKey, Value>, String> {
    let mut attrs = BTreeMap::new();
    for attr_result in e.attributes() {
        match attr_result {
            Ok(attr) => {
                let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                let val = match attr.unescape_value() {
                    Ok(v) => v.into_owned(),
                    Err(e) => return Err(format!("xml/parse: attribute decode error: {}", e)),
                };
                attrs.insert(TableKey::Keyword(key), Value::string(val.as_str()));
            }
            Err(e) => return Err(format!("xml/parse: attribute error: {}", e)),
        }
    }
    Ok(attrs)
}

fn element_to_value(elem: ParsedElement) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(
        TableKey::Keyword("tag".into()),
        Value::string(elem.tag.as_str()),
    );
    fields.insert(
        TableKey::Keyword("attrs".into()),
        Value::struct_from(elem.attrs),
    );
    fields.insert(
        TableKey::Keyword("children".into()),
        Value::array(elem.children),
    );
    Value::struct_from(fields)
}

/// Parse an XML string into an Elle element struct.
/// Returns the root element. If the document has multiple top-level elements,
/// returns the first one (XML requires a single root, but we're lenient).
fn parse_xml(input: &str) -> Result<Value, String> {
    let mut reader = Reader::from_reader(Cursor::new(input.as_bytes().to_vec()));
    reader.config_mut().trim_text(false);

    let mut stack: Vec<ParsedElement> = Vec::new();
    let mut buf = Vec::new();
    let mut roots: Vec<Value> = Vec::new();

    loop {
        buf.clear();
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let attrs = attrs_from_start(e)?;
                stack.push(ParsedElement {
                    tag,
                    attrs,
                    children: Vec::new(),
                });
            }
            Ok(Event::End(_)) => {
                let elem = match stack.pop() {
                    Some(e) => e,
                    None => return Err("xml/parse: unexpected closing tag".to_string()),
                };
                let value = element_to_value(elem);
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(value);
                } else {
                    roots.push(value);
                }
            }
            Ok(Event::Empty(ref e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                let attrs = attrs_from_start(e)?;
                let value = element_to_value(ParsedElement {
                    tag,
                    attrs,
                    children: Vec::new(),
                });
                if let Some(parent) = stack.last_mut() {
                    parent.children.push(value);
                } else {
                    roots.push(value);
                }
            }
            Ok(Event::Text(ref e)) => {
                let text = match e.unescape() {
                    Ok(t) => t.into_owned(),
                    Err(err) => return Err(format!("xml/parse: text decode error: {}", err)),
                };
                // Only push non-empty text nodes
                if !text.is_empty() {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(Value::string(text.as_str()));
                    }
                }
            }
            Ok(Event::CData(ref e)) => {
                let text = match e.decode() {
                    Ok(t) => t.into_owned(),
                    Err(err) => return Err(format!("xml/parse: CDATA decode error: {}", err)),
                };
                if !text.is_empty() {
                    if let Some(parent) = stack.last_mut() {
                        parent.children.push(Value::string(text.as_str()));
                    }
                }
            }
            Ok(Event::Comment(_))
            | Ok(Event::PI(_))
            | Ok(Event::Decl(_))
            | Ok(Event::DocType(_)) => {
                // Skip comments, processing instructions, XML declarations, DOCTYPE
            }
            Ok(Event::Eof) => {
                // If there are unclosed tags, the document is malformed
                if !stack.is_empty() {
                    return Err(format!(
                        "xml/parse: unclosed element '{}'",
                        stack.last().unwrap().tag
                    ));
                }
                break;
            }
            Err(e) => return Err(format!("xml/parse: {}", e)),
        }
    }

    if roots.is_empty() {
        Err("xml/parse: empty document".to_string())
    } else {
        Ok(roots.into_iter().next().unwrap())
    }
}

// ---------------------------------------------------------------------------
// DOM emitter helpers
// ---------------------------------------------------------------------------

const MAX_EMIT_DEPTH: usize = 256;

fn emit_xml(val: &Value) -> Result<String, String> {
    let mut output = Vec::new();
    let mut writer = Writer::new(&mut output);
    emit_element(&mut writer, val, 0)?;
    String::from_utf8(output).map_err(|e| format!("xml/emit: UTF-8 error: {}", e))
}

fn emit_element(
    writer: &mut Writer<&mut Vec<u8>>,
    val: &Value,
    depth: usize,
) -> Result<(), String> {
    if depth > MAX_EMIT_DEPTH {
        return Err("xml/emit: document too deeply nested (max 256)".to_string());
    }

    // If it's a string, emit it as escaped text content
    if let Some(text) = val.with_string(|s| s.to_string()) {
        let escaped = quick_xml::escape::escape(text.as_str());
        writer
            .write_event(Event::Text(BytesText::from_escaped(escaped.as_ref())))
            .map_err(|e| format!("xml/emit: write error: {}", e))?;
        return Ok(());
    }

    // Must be an element struct with :tag, :attrs, :children
    let tag = get_string_field(val, "tag")?;

    let attrs_val = get_struct_field(val, "attrs")?;
    let children = get_array_field(val, "children")?;

    let mut start = BytesStart::new(tag.as_str());

    // Emit attributes (BTreeMap gives sorted, deterministic order)
    if let Some(attrs_map) = attrs_val.as_struct() {
        for (k, v) in attrs_map.iter() {
            let key_str = match k {
                TableKey::Keyword(s) => s.as_str().to_string(),
                TableKey::String(s) => s.as_str().to_string(),
                _ => continue,
            };
            let val_str = match v.with_string(|s| s.to_string()) {
                Some(s) => s,
                None => {
                    return Err(format!(
                        "xml/emit: attribute value for '{}' must be a string, got {}",
                        key_str,
                        v.type_name()
                    ))
                }
            };
            start.push_attribute((key_str.as_str(), val_str.as_str()));
        }
    }

    if children.is_empty() {
        // Self-closing tag
        writer
            .write_event(Event::Empty(start))
            .map_err(|e| format!("xml/emit: write error: {}", e))?;
    } else {
        writer
            .write_event(Event::Start(start))
            .map_err(|e| format!("xml/emit: write error: {}", e))?;
        for child in &children {
            emit_element(writer, child, depth + 1)?;
        }
        writer
            .write_event(Event::End(BytesEnd::new(tag.as_str())))
            .map_err(|e| format!("xml/emit: write error: {}", e))?;
    }

    Ok(())
}

fn get_string_field(val: &Value, field: &str) -> Result<String, String> {
    let s = val
        .as_struct()
        .ok_or_else(|| format!("xml/emit: expected struct, got {}", val.type_name()))?;
    match s.get(&TableKey::Keyword(field.into())) {
        Some(v) => v.with_string(|s| s.to_string()).ok_or_else(|| {
            format!(
                "xml/emit: field '{}' must be a string, got {}",
                field,
                v.type_name()
            )
        }),
        None => Err(format!("xml/emit: missing field '{}'", field)),
    }
}

fn get_struct_field(val: &Value, field: &str) -> Result<Value, String> {
    let s = val
        .as_struct()
        .ok_or_else(|| format!("xml/emit: expected struct, got {}", val.type_name()))?;
    match s.get(&TableKey::Keyword(field.into())) {
        Some(v) => {
            if v.as_struct().is_some() || v.as_struct_mut().is_some() {
                Ok(*v)
            } else {
                Err(format!(
                    "xml/emit: field '{}' must be a struct, got {}",
                    field,
                    v.type_name()
                ))
            }
        }
        None => Err(format!("xml/emit: missing field '{}'", field)),
    }
}

fn get_array_field(val: &Value, field: &str) -> Result<Vec<Value>, String> {
    let s = val
        .as_struct()
        .ok_or_else(|| format!("xml/emit: expected struct, got {}", val.type_name()))?;
    match s.get(&TableKey::Keyword(field.into())) {
        Some(v) => {
            if let Some(arr) = v.as_array() {
                Ok(arr.to_vec())
            } else if let Some(arr_mut) = v.as_array_mut() {
                Ok(arr_mut.borrow().clone())
            } else {
                Err(format!(
                    "xml/emit: field '{}' must be an array, got {}",
                    field,
                    v.type_name()
                ))
            }
        }
        None => Err(format!("xml/emit: missing field '{}'", field)),
    }
}

// ---------------------------------------------------------------------------
// Streaming reader
// ---------------------------------------------------------------------------

/// Internal state for the streaming XML reader handle.
/// Owns the input via Cursor so no lifetime issues across calls.
/// `reader` and `buf` are in separate `RefCell`s so that `prim_xml_next_event`
/// can borrow them independently — `RefMut<Struct>` does not allow field
/// splitting, so a single `RefCell<XmlReaderState>` would cause a borrow error
/// when passing `&mut buf` to `reader.read_event_into`.
struct XmlReaderState {
    reader: RefCell<Reader<Cursor<Vec<u8>>>>,
    buf: RefCell<Vec<u8>>,
}

fn attrs_from_start_streaming(
    e: &BytesStart,
) -> Result<BTreeMap<TableKey, Value>, (SignalBits, Value)> {
    let mut attrs = BTreeMap::new();
    for attr_result in e.attributes() {
        match attr_result {
            Ok(attr) => {
                let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                let val = match attr.unescape_value() {
                    Ok(v) => v.into_owned(),
                    Err(err) => {
                        return Err((
                            SIG_ERROR,
                            error_val(
                                "xml-error",
                                format!("xml/next-event: attribute decode: {}", err),
                            ),
                        ));
                    }
                };
                attrs.insert(TableKey::Keyword(key), Value::string(val.as_str()));
            }
            Err(e) => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "xml-error",
                        format!("xml/next-event: attribute error: {}", e),
                    ),
                ));
            }
        }
    }
    Ok(attrs)
}

fn make_event_struct(fields: BTreeMap<TableKey, Value>) -> Value {
    Value::struct_from(fields)
}

fn prim_xml_reader_new(args: &[Value]) -> (SignalBits, Value) {
    let s = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "xml/reader-new: expected string, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    let cursor = Cursor::new(s.into_bytes());
    let mut reader = Reader::from_reader(cursor);
    reader.config_mut().trim_text(false);
    let state = XmlReaderState {
        reader: RefCell::new(reader),
        buf: RefCell::new(Vec::new()),
    };
    (SIG_OK, Value::external("xml-reader", state))
}

fn prim_xml_next_event(args: &[Value]) -> (SignalBits, Value) {
    let state = match args[0].as_external::<XmlReaderState>() {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "xml/next-event: expected xml-reader, got {}",
                        args[0].type_name()
                    ),
                ),
            );
        }
    };
    loop {
        // Parse one event, extracting all data into owned values before
        // the borrow of `buf` ends. The Event<'_> borrows from buf,
        // so we must extract all strings before the borrow expires.
        enum OwnedEvent {
            Start {
                tag: String,
                attrs: Result<BTreeMap<TableKey, Value>, (SignalBits, Value)>,
            },
            End {
                tag: String,
            },
            Text(String),
            Eof,
            Skip,
            Error(String),
        }
        let owned = {
            let mut buf = state.buf.borrow_mut();
            buf.clear();
            let mut reader = state.reader.borrow_mut();
            match reader.read_event_into(&mut buf) {
                Err(e) => OwnedEvent::Error(format!("xml/next-event: {}", e)),
                Ok(Event::Start(ref e)) => {
                    let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                    let attrs = attrs_from_start_streaming(e);
                    OwnedEvent::Start { tag, attrs }
                }
                Ok(Event::Empty(ref e)) => {
                    let tag = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                    let attrs = attrs_from_start_streaming(e);
                    OwnedEvent::Start { tag, attrs }
                }
                Ok(Event::End(ref e)) => OwnedEvent::End {
                    tag: String::from_utf8_lossy(e.name().as_ref()).into_owned(),
                },
                Ok(Event::Text(ref e)) => match e.unescape() {
                    Err(err) => OwnedEvent::Error(format!("xml/next-event: text decode: {}", err)),
                    Ok(t) => {
                        let text = t.into_owned();
                        if text.trim().is_empty() {
                            OwnedEvent::Skip
                        } else {
                            OwnedEvent::Text(text)
                        }
                    }
                },
                Ok(Event::CData(ref e)) => match e.decode() {
                    Err(err) => OwnedEvent::Error(format!("xml/next-event: CDATA decode: {}", err)),
                    Ok(t) => OwnedEvent::Text(t.into_owned()),
                },
                Ok(Event::Eof) => OwnedEvent::Eof,
                Ok(Event::Comment(_))
                | Ok(Event::PI(_))
                | Ok(Event::Decl(_))
                | Ok(Event::DocType(_)) => OwnedEvent::Skip,
            }
        };
        match owned {
            OwnedEvent::Error(msg) => {
                return (SIG_ERROR, error_val("xml-error", msg));
            }
            OwnedEvent::Start { tag, attrs } => {
                let attrs = match attrs {
                    Ok(a) => a,
                    Err(err) => return err,
                };
                let mut fields = BTreeMap::new();
                fields.insert(TableKey::Keyword("type".into()), Value::keyword("start"));
                fields.insert(TableKey::Keyword("tag".into()), Value::string(tag.as_str()));
                fields.insert(TableKey::Keyword("attrs".into()), Value::struct_from(attrs));
                return (SIG_OK, make_event_struct(fields));
            }
            OwnedEvent::End { tag } => {
                let mut fields = BTreeMap::new();
                fields.insert(TableKey::Keyword("type".into()), Value::keyword("end"));
                fields.insert(TableKey::Keyword("tag".into()), Value::string(tag.as_str()));
                return (SIG_OK, make_event_struct(fields));
            }
            OwnedEvent::Text(text) => {
                let mut fields = BTreeMap::new();
                fields.insert(TableKey::Keyword("type".into()), Value::keyword("text"));
                fields.insert(
                    TableKey::Keyword("content".into()),
                    Value::string(text.as_str()),
                );
                return (SIG_OK, make_event_struct(fields));
            }
            OwnedEvent::Eof => {
                let mut fields = BTreeMap::new();
                fields.insert(TableKey::Keyword("type".into()), Value::keyword("eof"));
                return (SIG_OK, make_event_struct(fields));
            }
            OwnedEvent::Skip => continue,
        }
    }
}

fn prim_xml_reader_close(args: &[Value]) -> (SignalBits, Value) {
    // Validate type; the reader itself is freed when the Value is GC'd.
    match args[0].as_external::<XmlReaderState>() {
        Some(_) => (SIG_OK, Value::NIL),
        None => (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "xml/reader-close: expected xml-reader, got {}",
                    args[0].type_name()
                ),
            ),
        ),
    }
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_xml_parse(args: &[Value]) -> (SignalBits, Value) {
    let s = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("xml/parse: expected string, got {}", args[0].type_name()),
                ),
            );
        }
    };
    match parse_xml(&s) {
        Ok(val) => (SIG_OK, val),
        Err(e) => (SIG_ERROR, error_val("xml-error", e)),
    }
}

fn prim_xml_emit(args: &[Value]) -> (SignalBits, Value) {
    // The top-level argument must be an element struct — strings are valid
    // child nodes inside a document, but not valid document roots.
    if args[0].as_struct().is_none() {
        return (
            SIG_ERROR,
            error_val(
                "xml-error",
                format!(
                    "xml/emit: expected element struct, got {}",
                    args[0].type_name()
                ),
            ),
        );
    }
    match emit_xml(&args[0]) {
        Ok(s) => (SIG_OK, Value::string(s.as_str())),
        Err(e) => (SIG_ERROR, error_val("xml-error", e)),
    }
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "xml/parse",
        func: prim_xml_parse,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse an XML string into a nested struct/array tree",
        params: &["xml-string"],
        category: "xml",
        example: r#"(xml/parse "<root><child>text</child></root>")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "xml/emit",
        func: prim_xml_emit,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Serialize an element struct tree to an XML string",
        params: &["element"],
        category: "xml",
        example: r#"(xml/emit {:tag "root" :attrs {} :children []})"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "xml/reader-new",
        func: prim_xml_reader_new,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create a streaming XML reader from a string",
        params: &["xml-string"],
        category: "xml",
        example: r#"(xml/reader-new "<root/>")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "xml/next-event",
        func: prim_xml_next_event,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Read the next event from a streaming XML reader",
        params: &["reader"],
        category: "xml",
        example: "(xml/next-event reader)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "xml/reader-close",
        func: prim_xml_reader_close,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Close a streaming XML reader (validates type; reader is freed with the value)",
        params: &["reader"],
        category: "xml",
        example: "(xml/reader-close reader)",
        aliases: &[],
    },
];
