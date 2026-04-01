//! Elle Arrow plugin — Apache Arrow columnar data via the `arrow` and `parquet` crates.

use std::collections::BTreeMap;
use std::io::Cursor;
use std::sync::Arc;

use arrow::array::{
    Array, ArrayRef, BooleanArray, Float64Array, Int64Array, NullArray, RecordBatch, StringArray,
};
use arrow::datatypes::{DataType, Field, Schema};
use arrow::ipc::reader::StreamReader;
use arrow::ipc::writer::StreamWriter;
use arrow::util::pretty::pretty_format_batches;
use parquet::arrow::arrow_reader::ParquetRecordBatchReaderBuilder;
use parquet::arrow::ArrowWriter;

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::{Arity, TableKey};
use elle::value::{error_val, Value};

// ---------------------------------------------------------------------------
// Type wrappers
// ---------------------------------------------------------------------------

/// Wrapped RecordBatch stored as an external value.
struct BatchWrap(RecordBatch);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_batch<'a>(val: &'a Value, name: &str) -> Result<&'a BatchWrap, (SignalBits, Value)> {
    val.as_external::<BatchWrap>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected arrow/batch, got {}", name, val.type_name()),
            ),
        )
    })
}

fn extract_string(val: &Value, name: &str) -> Result<String, (SignalBits, Value)> {
    val.with_string(|s| s.to_owned()).ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected string, got {}", name, val.type_name()),
            ),
        )
    })
}

/// Convert an Elle array of values into an Arrow ArrayRef by inferring types.
fn elle_values_to_arrow(values: &[Value], field_name: &str) -> Result<ArrayRef, String> {
    if values.is_empty() {
        return Ok(Arc::new(NullArray::new(0)));
    }

    // Infer type from first non-nil value
    let first_non_nil = values.iter().find(|v| !v.is_nil());
    match first_non_nil {
        None => Ok(Arc::new(NullArray::new(values.len()))),
        Some(v) if v.as_int().is_some() => {
            let arr: Int64Array = values.iter().map(|v| v.as_int()).collect();
            Ok(Arc::new(arr))
        }
        Some(v) if v.as_float().is_some() => {
            let arr: Float64Array = values.iter().map(|v| v.as_float()).collect();
            Ok(Arc::new(arr))
        }
        Some(v) if v.as_bool().is_some() => {
            let arr: BooleanArray = values.iter().map(|v| v.as_bool()).collect();
            Ok(Arc::new(arr))
        }
        Some(v) if v.with_string(|_| ()).is_some() => {
            let strings: Vec<Option<String>> = values
                .iter()
                .map(|v| v.with_string(|s| s.to_owned()))
                .collect();
            let arr: StringArray = strings.iter().map(|s| s.as_deref()).collect();
            Ok(Arc::new(arr))
        }
        _ => Err(format!(
            "cannot convert column '{}' to Arrow: unsupported element type",
            field_name
        )),
    }
}

/// Convert an Arrow array to a Vec<Value>.
fn arrow_to_elle_values(arr: &dyn Array) -> Vec<Value> {
    let len = arr.len();
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        if arr.is_null(i) {
            out.push(Value::NIL);
        } else {
            match arr.data_type() {
                DataType::Int8 | DataType::Int16 | DataType::Int32 | DataType::Int64 => {
                    let a = arr.as_any().downcast_ref::<Int64Array>().or(None);
                    if let Some(a) = a {
                        out.push(Value::int(a.value(i)));
                    } else {
                        // Try to cast
                        let casted = arrow::compute::cast(arr, &DataType::Int64).ok();
                        if let Some(ref c) = casted {
                            let a = c.as_any().downcast_ref::<Int64Array>().unwrap();
                            out.push(Value::int(a.value(i)));
                        } else {
                            out.push(Value::string(format!("{:?}", arr)));
                        }
                    }
                }
                DataType::UInt8 | DataType::UInt16 | DataType::UInt32 | DataType::UInt64 => {
                    let casted = arrow::compute::cast(arr, &DataType::Int64).ok();
                    if let Some(ref c) = casted {
                        let a = c.as_any().downcast_ref::<Int64Array>().unwrap();
                        out.push(Value::int(a.value(i)));
                    } else {
                        out.push(Value::string("<arrow-value>"));
                    }
                }
                DataType::Float16 | DataType::Float32 | DataType::Float64 => {
                    let casted = arrow::compute::cast(arr, &DataType::Float64).ok();
                    if let Some(ref c) = casted {
                        let a = c.as_any().downcast_ref::<Float64Array>().unwrap();
                        out.push(Value::float(a.value(i)));
                    } else {
                        out.push(Value::string("<arrow-value>"));
                    }
                }
                DataType::Boolean => {
                    let a = arr.as_any().downcast_ref::<BooleanArray>().unwrap();
                    out.push(Value::bool(a.value(i)));
                }
                DataType::Utf8 | DataType::LargeUtf8 => {
                    let a = arr.as_any().downcast_ref::<StringArray>();
                    if let Some(a) = a {
                        out.push(Value::string(a.value(i)));
                    } else {
                        out.push(Value::string(""));
                    }
                }
                _ => {
                    // Fallback: stringify
                    let formatted =
                        arrow::util::display::ArrayFormatter::try_new(arr, &Default::default());
                    if let Ok(f) = formatted {
                        out.push(Value::string(f.value(i).to_string()));
                    } else {
                        out.push(Value::string("<arrow-value>"));
                    }
                }
            }
        }
    }
    out
}

/// Convert a RecordBatch to an Elle array of structs.
fn batch_to_elle(batch: &RecordBatch) -> Value {
    let schema = batch.schema();
    let num_rows = batch.num_rows();
    let mut rows: Vec<Value> = Vec::with_capacity(num_rows);

    // Pre-convert all columns
    let columns: Vec<Vec<Value>> = batch
        .columns()
        .iter()
        .map(|col| arrow_to_elle_values(col.as_ref()))
        .collect();

    for row_idx in 0..num_rows {
        let mut fields = BTreeMap::new();
        for (col_vals, field) in columns.iter().zip(schema.fields().iter()) {
            fields.insert(TableKey::Keyword(field.name().clone()), col_vals[row_idx]);
        }
        rows.push(Value::struct_from(fields));
    }
    Value::array(rows)
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

/// (arrow/batch columns) — create a RecordBatch from a struct of column-name → array mappings.
fn prim_batch(args: &[Value]) -> (SignalBits, Value) {
    let name = "arrow/batch";

    // Accept a struct where keys are column names and values are arrays
    let columns: Vec<(String, Vec<Value>)> = if let Some(map) = args[0].as_struct() {
        map.iter()
            .filter_map(|(k, v)| {
                if let TableKey::Keyword(s) = k {
                    let vals = if let Some(arr) = v.as_array() {
                        arr.to_vec()
                    } else if let Some(arr_ref) = v.as_array_mut() {
                        arr_ref.borrow().clone()
                    } else {
                        return None;
                    };
                    Some((s.clone(), vals))
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
                format!("{}: expected struct of column arrays", name),
            ),
        );
    };

    if columns.is_empty() {
        return (
            SIG_ERROR,
            error_val("arrow-error", format!("{}: no columns provided", name)),
        );
    }

    let mut fields = Vec::new();
    let mut arrays: Vec<ArrayRef> = Vec::new();

    for (col_name, values) in &columns {
        match elle_values_to_arrow(values, col_name) {
            Ok(arr) => {
                fields.push(Field::new(col_name, arr.data_type().clone(), true));
                arrays.push(arr);
            }
            Err(e) => {
                return (
                    SIG_ERROR,
                    error_val("arrow-error", format!("{}: {}", name, e)),
                )
            }
        }
    }

    let schema = Arc::new(Schema::new(fields));
    match RecordBatch::try_new(schema, arrays) {
        Ok(batch) => (SIG_OK, Value::external("arrow/batch", BatchWrap(batch))),
        Err(e) => (
            SIG_ERROR,
            error_val("arrow-error", format!("{}: {}", name, e)),
        ),
    }
}

/// (arrow/schema batch) — return the schema of a batch as a struct.
fn prim_schema(args: &[Value]) -> (SignalBits, Value) {
    let name = "arrow/schema";
    let batch = match get_batch(&args[0], name) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let schema = batch.0.schema();
    let mut fields = BTreeMap::new();
    for field in schema.fields() {
        fields.insert(
            TableKey::Keyword(field.name().clone()),
            Value::string(format!("{}", field.data_type())),
        );
    }
    (SIG_OK, Value::struct_from(fields))
}

/// (arrow/num-rows batch) — return number of rows.
fn prim_num_rows(args: &[Value]) -> (SignalBits, Value) {
    let name = "arrow/num-rows";
    let batch = match get_batch(&args[0], name) {
        Ok(b) => b,
        Err(e) => return e,
    };
    (SIG_OK, Value::int(batch.0.num_rows() as i64))
}

/// (arrow/num-cols batch) — return number of columns.
fn prim_num_cols(args: &[Value]) -> (SignalBits, Value) {
    let name = "arrow/num-cols";
    let batch = match get_batch(&args[0], name) {
        Ok(b) => b,
        Err(e) => return e,
    };
    (SIG_OK, Value::int(batch.0.num_columns() as i64))
}

/// (arrow/column batch col-name) — extract a column as an Elle array.
fn prim_column(args: &[Value]) -> (SignalBits, Value) {
    let name = "arrow/column";
    let batch = match get_batch(&args[0], name) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let col_name = match extract_string(&args[1], name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let schema = batch.0.schema();
    match schema.index_of(&col_name) {
        Ok(idx) => {
            let col = batch.0.column(idx);
            let values = arrow_to_elle_values(col.as_ref());
            (SIG_OK, Value::array(values))
        }
        Err(_) => (
            SIG_ERROR,
            error_val(
                "arrow-error",
                format!("{}: column '{}' not found", name, col_name),
            ),
        ),
    }
}

/// (arrow/to-rows batch) — convert a RecordBatch to an Elle array of structs.
fn prim_to_rows(args: &[Value]) -> (SignalBits, Value) {
    let name = "arrow/to-rows";
    let batch = match get_batch(&args[0], name) {
        Ok(b) => b,
        Err(e) => return e,
    };
    (SIG_OK, batch_to_elle(&batch.0))
}

/// (arrow/display batch) — pretty-print a RecordBatch as a table string.
fn prim_display(args: &[Value]) -> (SignalBits, Value) {
    let name = "arrow/display";
    let batch = match get_batch(&args[0], name) {
        Ok(b) => b,
        Err(e) => return e,
    };
    match pretty_format_batches(std::slice::from_ref(&batch.0)) {
        Ok(table) => (SIG_OK, Value::string(table.to_string())),
        Err(e) => (
            SIG_ERROR,
            error_val("arrow-error", format!("{}: {}", name, e)),
        ),
    }
}

/// (arrow/write-ipc batch) — serialize a RecordBatch to IPC bytes.
fn prim_write_ipc(args: &[Value]) -> (SignalBits, Value) {
    let name = "arrow/write-ipc";
    let batch = match get_batch(&args[0], name) {
        Ok(b) => b,
        Err(e) => return e,
    };

    let mut buf = Vec::new();
    let schema = batch.0.schema();
    let mut writer = match StreamWriter::try_new(&mut buf, &schema) {
        Ok(w) => w,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("arrow-error", format!("{}: {}", name, e)),
            )
        }
    };
    if let Err(e) = writer.write(&batch.0) {
        return (
            SIG_ERROR,
            error_val("arrow-error", format!("{}: {}", name, e)),
        );
    }
    if let Err(e) = writer.finish() {
        return (
            SIG_ERROR,
            error_val("arrow-error", format!("{}: {}", name, e)),
        );
    }
    (SIG_OK, Value::bytes(buf))
}

/// (arrow/read-ipc bytes) — deserialize IPC bytes to a RecordBatch.
fn prim_read_ipc(args: &[Value]) -> (SignalBits, Value) {
    let name = "arrow/read-ipc";
    let bytes = args[0].as_bytes().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected bytes, got {}", name, args[0].type_name()),
            ),
        )
    });
    let bytes = match bytes {
        Ok(b) => b,
        Err(e) => return e,
    };

    let cursor = Cursor::new(bytes.to_vec());
    let reader = match StreamReader::try_new(cursor, None) {
        Ok(r) => r,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("arrow-error", format!("{}: {}", name, e)),
            )
        }
    };

    let mut batches = Vec::new();
    for batch_result in reader {
        match batch_result {
            Ok(batch) => batches.push(batch),
            Err(e) => {
                return (
                    SIG_ERROR,
                    error_val("arrow-error", format!("{}: {}", name, e)),
                )
            }
        }
    }

    if batches.len() == 1 {
        (
            SIG_OK,
            Value::external(
                "arrow/batch",
                BatchWrap(batches.into_iter().next().unwrap()),
            ),
        )
    } else {
        let vals: Vec<Value> = batches
            .into_iter()
            .map(|b| Value::external("arrow/batch", BatchWrap(b)))
            .collect();
        (SIG_OK, Value::array(vals))
    }
}

/// (arrow/write-parquet batch) — serialize a RecordBatch to Parquet bytes.
fn prim_write_parquet(args: &[Value]) -> (SignalBits, Value) {
    let name = "arrow/write-parquet";
    let batch = match get_batch(&args[0], name) {
        Ok(b) => b,
        Err(e) => return e,
    };

    let mut buf = Vec::new();
    let schema = batch.0.schema();
    let mut writer = match ArrowWriter::try_new(&mut buf, schema, None) {
        Ok(w) => w,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("arrow-error", format!("{}: {}", name, e)),
            )
        }
    };
    if let Err(e) = writer.write(&batch.0) {
        return (
            SIG_ERROR,
            error_val("arrow-error", format!("{}: {}", name, e)),
        );
    }
    if let Err(e) = writer.close() {
        return (
            SIG_ERROR,
            error_val("arrow-error", format!("{}: {}", name, e)),
        );
    }
    (SIG_OK, Value::bytes(buf))
}

/// (arrow/read-parquet bytes) — deserialize Parquet bytes to a RecordBatch.
fn prim_read_parquet(args: &[Value]) -> (SignalBits, Value) {
    let name = "arrow/read-parquet";
    let bytes = args[0].as_bytes().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected bytes, got {}", name, args[0].type_name()),
            ),
        )
    });
    let bytes = match bytes {
        Ok(b) => b,
        Err(e) => return e,
    };

    let builder = match ParquetRecordBatchReaderBuilder::try_new(bytes::Bytes::from(bytes.to_vec()))
    {
        Ok(b) => b,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("arrow-error", format!("{}: {}", name, e)),
            )
        }
    };
    let reader = match builder.build() {
        Ok(r) => r,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val("arrow-error", format!("{}: {}", name, e)),
            )
        }
    };

    let mut batches = Vec::new();
    for batch_result in reader {
        match batch_result {
            Ok(batch) => batches.push(batch),
            Err(e) => {
                return (
                    SIG_ERROR,
                    error_val("arrow-error", format!("{}: {}", name, e)),
                )
            }
        }
    }

    if batches.is_empty() {
        return (
            SIG_ERROR,
            error_val("arrow-error", format!("{}: no data in parquet", name)),
        );
    }

    // Concatenate all batches
    if batches.len() == 1 {
        (
            SIG_OK,
            Value::external(
                "arrow/batch",
                BatchWrap(batches.into_iter().next().unwrap()),
            ),
        )
    } else {
        let schema = batches[0].schema();
        match arrow::compute::concat_batches(&schema, &batches) {
            Ok(merged) => (SIG_OK, Value::external("arrow/batch", BatchWrap(merged))),
            Err(e) => (
                SIG_ERROR,
                error_val("arrow-error", format!("{}: {}", name, e)),
            ),
        }
    }
}

/// (arrow/slice batch offset length) — take a zero-copy slice of a batch.
fn prim_slice(args: &[Value]) -> (SignalBits, Value) {
    let name = "arrow/slice";
    let batch = match get_batch(&args[0], name) {
        Ok(b) => b,
        Err(e) => return e,
    };
    let offset = args[1].as_int().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val("type-error", format!("{}: offset must be integer", name)),
        )
    });
    let offset = match offset {
        Ok(o) => o as usize,
        Err(e) => return e,
    };
    let length = args[2].as_int().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val("type-error", format!("{}: length must be integer", name)),
        )
    });
    let length = match length {
        Ok(l) => l as usize,
        Err(e) => return e,
    };

    let sliced = batch.0.slice(offset, length);
    (SIG_OK, Value::external("arrow/batch", BatchWrap(sliced)))
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "arrow/batch",
        func: prim_batch,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create a RecordBatch from a struct of column-name → array mappings. Values are typed by inference (int, float, bool, string).",
        params: &["columns"],
        category: "arrow",
        example: r#"(arrow/batch {:name ["Alice" "Bob"] :age [30 25]})"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "arrow/schema",
        func: prim_schema,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the schema of a batch as a struct mapping column names to type strings.",
        params: &["batch"],
        category: "arrow",
        example: r#"(arrow/schema my-batch)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "arrow/num-rows",
        func: prim_num_rows,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the number of rows in a batch.",
        params: &["batch"],
        category: "arrow",
        example: "(arrow/num-rows my-batch)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arrow/num-cols",
        func: prim_num_cols,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the number of columns in a batch.",
        params: &["batch"],
        category: "arrow",
        example: "(arrow/num-cols my-batch)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arrow/column",
        func: prim_column,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Extract a column from a batch by name, returned as an Elle array.",
        params: &["batch", "column-name"],
        category: "arrow",
        example: r#"(arrow/column my-batch "name")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "arrow/to-rows",
        func: prim_to_rows,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert a RecordBatch to an Elle array of structs (one struct per row).",
        params: &["batch"],
        category: "arrow",
        example: "(arrow/to-rows my-batch)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arrow/display",
        func: prim_display,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Pretty-print a RecordBatch as a formatted table string.",
        params: &["batch"],
        category: "arrow",
        example: "(arrow/display my-batch)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arrow/write-ipc",
        func: prim_write_ipc,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Serialize a RecordBatch to Arrow IPC stream format (bytes).",
        params: &["batch"],
        category: "arrow",
        example: "(arrow/write-ipc my-batch)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arrow/read-ipc",
        func: prim_read_ipc,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Deserialize Arrow IPC bytes into a RecordBatch (or array of batches).",
        params: &["bytes"],
        category: "arrow",
        example: "(arrow/read-ipc ipc-bytes)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arrow/write-parquet",
        func: prim_write_parquet,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Serialize a RecordBatch to Parquet format (bytes).",
        params: &["batch"],
        category: "arrow",
        example: "(arrow/write-parquet my-batch)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arrow/read-parquet",
        func: prim_read_parquet,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Deserialize Parquet bytes into a RecordBatch.",
        params: &["bytes"],
        category: "arrow",
        example: "(arrow/read-parquet pq-bytes)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "arrow/slice",
        func: prim_slice,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Take a zero-copy slice of a batch: (arrow/slice batch offset length).",
        params: &["batch", "offset", "length"],
        category: "arrow",
        example: "(arrow/slice my-batch 0 10)",
        aliases: &[],
    },
];
elle::elle_plugin_init!(PRIMITIVES, "arrow/");
