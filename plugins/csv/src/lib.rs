//! Elle CSV plugin — CSV parsing and serialization via the `csv` crate.

use std::collections::BTreeMap;

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::{Arity, TableKey};
use elle::value::{error_val, Value};
elle::elle_plugin_init!(PRIMITIVES, "csv/");

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract a string from a Value (immutable only — CSV text must be valid UTF-8 string).
fn extract_string(val: &Value, name: &str) -> Result<String, (SignalBits, Value)> {
    if let Some(s) = val.with_string(|s| s.to_owned()) {
        return Ok(s);
    }
    Err((
        SIG_ERROR,
        error_val(
            "type-error",
            format!("{}: expected string, got {}", name, val.type_name()),
        ),
    ))
}

/// Extract the delimiter byte from an opts struct (second argument).
/// Returns b',' if opts is nil or absent. Returns an error if opts is present
/// but malformed.
fn extract_delimiter(opts: &Value, name: &str) -> Result<u8, (SignalBits, Value)> {
    if opts.is_nil() {
        return Ok(b',');
    }
    // opts must be a struct (immutable or mutable)
    let delim_val = if let Some(map) = opts.as_struct() {
        elle::value::sorted_struct_get(map, &TableKey::Keyword("delimiter".into())).copied()
    } else if let Some(map_ref) = opts.as_struct_mut() {
        map_ref
            .borrow()
            .get(&TableKey::Keyword("delimiter".into()))
            .copied()
    } else {
        return Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: opts must be a struct, got {}", name, opts.type_name()),
            ),
        ));
    };

    match delim_val {
        None => Ok(b','),
        Some(v) => {
            let s = v.with_string(|s| s.to_owned()).ok_or_else(|| {
                (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "{}: :delimiter must be a single-character string, got {}",
                            name,
                            v.type_name()
                        ),
                    ),
                )
            })?;
            let b = s.as_bytes();
            if b.len() != 1 {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "csv-error",
                        format!(
                            "{}: :delimiter must be a single-character string, got {:?}",
                            name, s
                        ),
                    ),
                ));
            }
            Ok(b[0])
        }
    }
}

/// Stringify a Value for CSV output. Strings are written as-is; everything
/// else uses the Display representation.
fn value_to_csv_field(val: Value) -> String {
    if let Some(s) = val.with_string(|s| s.to_owned()) {
        return s;
    }
    format!("{}", val)
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_csv_parse(args: &[Value]) -> (SignalBits, Value) {
    let name = "csv/parse";
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("{}: expected 1 or 2 arguments, got {}", name, args.len()),
            ),
        );
    }
    let text = match extract_string(&args[0], name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let opts = if args.len() == 2 { args[1] } else { Value::NIL };
    let delim = match extract_delimiter(&opts, name) {
        Ok(d) => d,
        Err(e) => return e,
    };

    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delim)
        .from_reader(text.as_bytes());

    let headers: Vec<String> = match rdr.headers() {
        Ok(rec) => rec.iter().map(|s| s.to_owned()).collect(),
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("csv-error", format!("{}: {}", name, e)),
            )
        }
    };

    let mut rows: Vec<Value> = Vec::new();
    for result in rdr.records() {
        let record = match result {
            Ok(r) => r,
            Err(e) => {
                return (
                    SIG_ERROR,
                    error_val("csv-error", format!("{}: {}", name, e)),
                )
            }
        };
        let mut fields: BTreeMap<TableKey, Value> = BTreeMap::new();
        for (header, field) in headers.iter().zip(record.iter()) {
            fields.insert(TableKey::Keyword(header.clone()), Value::string(field));
        }
        rows.push(Value::struct_from(fields));
    }
    (SIG_OK, Value::array(rows))
}

fn prim_csv_parse_rows(args: &[Value]) -> (SignalBits, Value) {
    let name = "csv/parse-rows";
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("{}: expected 1 or 2 arguments, got {}", name, args.len()),
            ),
        );
    }
    let text = match extract_string(&args[0], name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let opts = if args.len() == 2 { args[1] } else { Value::NIL };
    let delim = match extract_delimiter(&opts, name) {
        Ok(d) => d,
        Err(e) => return e,
    };

    let mut rdr = csv::ReaderBuilder::new()
        .delimiter(delim)
        .has_headers(false)
        .from_reader(text.as_bytes());

    let mut rows: Vec<Value> = Vec::new();
    for result in rdr.records() {
        let record = match result {
            Ok(r) => r,
            Err(e) => {
                return (
                    SIG_ERROR,
                    error_val("csv-error", format!("{}: {}", name, e)),
                )
            }
        };
        let fields: Vec<Value> = record.iter().map(Value::string).collect();
        rows.push(Value::array(fields));
    }
    (SIG_OK, Value::array(rows))
}

fn prim_csv_write(args: &[Value]) -> (SignalBits, Value) {
    let name = "csv/write";
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("{}: expected 1 or 2 arguments, got {}", name, args.len()),
            ),
        );
    }

    // rows must be an array (immutable or mutable)
    let rows: Vec<Value> = if let Some(arr) = args[0].as_array() {
        arr.to_vec()
    } else if let Some(arr_ref) = args[0].as_array_mut() {
        arr_ref.borrow().clone()
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected array, got {}", name, args[0].type_name()),
            ),
        );
    };

    let opts = if args.len() == 2 { args[1] } else { Value::NIL };
    let delim = match extract_delimiter(&opts, name) {
        Ok(d) => d,
        Err(e) => return e,
    };

    let mut out: Vec<u8> = Vec::new();
    let mut wtr = csv::WriterBuilder::new()
        .delimiter(delim)
        .from_writer(&mut out);

    // Extract keys from first struct (BTreeMap order = alphabetical = stable)
    let keys: Vec<String> = if rows.is_empty() {
        vec![]
    } else {
        let first = rows[0];
        if let Some(map) = first.as_struct() {
            map.iter()
                .filter_map(|(k, _)| {
                    if let TableKey::Keyword(s) = k {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .collect()
        } else if let Some(map_ref) = first.as_struct_mut() {
            map_ref
                .borrow()
                .keys()
                .filter_map(|k| {
                    if let TableKey::Keyword(s) = k {
                        Some(s.clone())
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: each row must be a struct, got {}",
                        name,
                        first.type_name()
                    ),
                ),
            );
        }
    };

    // Write header row
    if let Err(e) = wtr.write_record(&keys) {
        return (
            SIG_ERROR,
            error_val("csv-error", format!("{}: {}", name, e)),
        );
    }

    // Write data rows
    for row in &rows {
        let record: Vec<String> = if let Some(map) = row.as_struct() {
            keys.iter()
                .map(|k| {
                    elle::value::sorted_struct_get(map, &TableKey::Keyword(k.clone()))
                        .map(|v| value_to_csv_field(*v))
                        .unwrap_or_default()
                })
                .collect()
        } else if let Some(map_ref) = row.as_struct_mut() {
            let map = map_ref.borrow();
            keys.iter()
                .map(|k| {
                    map.get(&TableKey::Keyword(k.clone()))
                        .map(|v| value_to_csv_field(*v))
                        .unwrap_or_default()
                })
                .collect()
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: each row must be a struct, got {}",
                        name,
                        row.type_name()
                    ),
                ),
            );
        };
        if let Err(e) = wtr.write_record(&record) {
            return (
                SIG_ERROR,
                error_val("csv-error", format!("{}: {}", name, e)),
            );
        }
    }

    drop(wtr);

    let s = match String::from_utf8(out) {
        Ok(s) => s,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("csv-error", format!("{}: {}", name, e)),
            )
        }
    };
    (SIG_OK, Value::string(s))
}

fn prim_csv_write_rows(args: &[Value]) -> (SignalBits, Value) {
    let name = "csv/write-rows";
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("{}: expected 1 or 2 arguments, got {}", name, args.len()),
            ),
        );
    }

    // rows must be an array (immutable or mutable)
    let rows: Vec<Value> = if let Some(arr) = args[0].as_array() {
        arr.to_vec()
    } else if let Some(arr_ref) = args[0].as_array_mut() {
        arr_ref.borrow().clone()
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected array, got {}", name, args[0].type_name()),
            ),
        );
    };

    let opts = if args.len() == 2 { args[1] } else { Value::NIL };
    let delim = match extract_delimiter(&opts, name) {
        Ok(d) => d,
        Err(e) => return e,
    };

    let mut out: Vec<u8> = Vec::new();
    let mut wtr = csv::WriterBuilder::new()
        .delimiter(delim)
        .from_writer(&mut out);

    for row in &rows {
        let fields: Vec<String> = if let Some(arr) = row.as_array() {
            arr.iter().map(|v| value_to_csv_field(*v)).collect()
        } else if let Some(arr_ref) = row.as_array_mut() {
            arr_ref
                .borrow()
                .iter()
                .map(|v| value_to_csv_field(*v))
                .collect()
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: each row must be an array, got {}",
                        name,
                        row.type_name()
                    ),
                ),
            );
        };
        if let Err(e) = wtr.write_record(&fields) {
            return (
                SIG_ERROR,
                error_val("csv-error", format!("{}: {}", name, e)),
            );
        }
    }

    drop(wtr);

    let s = match String::from_utf8(out) {
        Ok(s) => s,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("csv-error", format!("{}: {}", name, e)),
            )
        }
    };
    (SIG_OK, Value::string(s))
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "csv/parse",
        func: prim_csv_parse,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Parse a CSV string with headers. First row becomes keyword keys. Returns array of structs. Optional opts: {:delimiter char-string}.",
        params: &["text", "opts"],
        category: "csv",
        example: r#"(csv/parse "name,age\nAlice,30")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "csv/parse-rows",
        func: prim_csv_parse_rows,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Parse a CSV string without header interpretation. Returns array of arrays. Optional opts: {:delimiter char-string}.",
        params: &["text", "opts"],
        category: "csv",
        example: r#"(csv/parse-rows "a,b\n1,2")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "csv/write",
        func: prim_csv_write,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Serialize an array of structs to a CSV string. Keys from the first struct become the header row. Optional opts: {:delimiter char-string}.",
        params: &["rows", "opts"],
        category: "csv",
        example: r#"(csv/write [{:name "Alice" :age "30"}])"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "csv/write-rows",
        func: prim_csv_write_rows,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Serialize an array of arrays to a CSV string without headers. Optional opts: {:delimiter char-string}.",
        params: &["rows", "opts"],
        category: "csv",
        example: r#"(csv/write-rows [["a" "b"] ["1" "2"]])"#,
        aliases: &[],
    },
];
