//! Elle Polars plugin — DataFrame operations via the `polars` crate.

use std::collections::BTreeMap;
use std::io::Cursor;

use polars::prelude::*;

use elle::primitives::def::PrimitiveDef;
use elle::signals::Signal;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::{Arity, TableKey};
use elle::value::{error_val, Value};

// ---------------------------------------------------------------------------
// Type wrapper
// ---------------------------------------------------------------------------

/// Wrapped DataFrame stored as an external value.
struct DfWrap(DataFrame);

/// Wrapped LazyFrame stored as an external value.
struct LazyWrap(LazyFrame);

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn get_df<'a>(val: &'a Value, name: &str) -> Result<&'a DfWrap, (SignalBits, Value)> {
    val.as_external::<DfWrap>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected polars/df, got {}", name, val.type_name()),
            ),
        )
    })
}

fn get_lazy<'a>(val: &'a Value, name: &str) -> Result<&'a LazyWrap, (SignalBits, Value)> {
    val.as_external::<LazyWrap>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected polars/lazy, got {}", name, val.type_name()),
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

fn extract_string_list(val: &Value, name: &str) -> Result<Vec<String>, (SignalBits, Value)> {
    let arr = if let Some(a) = val.as_array() {
        a.to_vec()
    } else if let Some(a) = val.as_array_mut() {
        a.borrow().clone()
    } else {
        return Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected array of strings, got {}",
                    name,
                    val.type_name()
                ),
            ),
        ));
    };
    arr.iter().map(|v| extract_string(v, name)).collect()
}

/// Convert a Polars Series to an Elle array of values.
fn series_to_elle(s: &Series) -> Vec<Value> {
    let len = s.len();
    let mut out = Vec::with_capacity(len);

    for i in 0..len {
        let val = s.get(i);
        match val {
            Ok(AnyValue::Null) => out.push(Value::NIL),
            Ok(AnyValue::Boolean(b)) => out.push(Value::bool(b)),
            Ok(AnyValue::Int8(v)) => out.push(Value::int(v as i64)),
            Ok(AnyValue::Int16(v)) => out.push(Value::int(v as i64)),
            Ok(AnyValue::Int32(v)) => out.push(Value::int(v as i64)),
            Ok(AnyValue::Int64(v)) => out.push(Value::int(v)),
            Ok(AnyValue::UInt8(v)) => out.push(Value::int(v as i64)),
            Ok(AnyValue::UInt16(v)) => out.push(Value::int(v as i64)),
            Ok(AnyValue::UInt32(v)) => out.push(Value::int(v as i64)),
            Ok(AnyValue::UInt64(v)) => out.push(Value::int(v as i64)),
            Ok(AnyValue::Float32(v)) => out.push(Value::float(v as f64)),
            Ok(AnyValue::Float64(v)) => out.push(Value::float(v)),
            Ok(AnyValue::String(s)) => out.push(Value::string(s)),
            Ok(other) => out.push(Value::string(format!("{}", other))),
            Err(_) => out.push(Value::NIL),
        }
    }
    out
}

/// Convert a DataFrame to an Elle array of structs.
fn df_to_elle(df: &DataFrame) -> Value {
    let num_rows = df.height();
    let columns: Vec<(&str, Vec<Value>)> = df
        .get_columns()
        .iter()
        .map(|s| {
            (
                s.name().as_str(),
                series_to_elle(s.as_materialized_series()),
            )
        })
        .collect();

    let mut rows: Vec<Value> = Vec::with_capacity(num_rows);
    for i in 0..num_rows {
        let mut fields = BTreeMap::new();
        for (col_name, col_vals) in &columns {
            fields.insert(TableKey::Keyword(col_name.to_string()), col_vals[i]);
        }
        rows.push(Value::struct_from(fields));
    }
    Value::array(rows)
}

/// Build a Vec<Series> from an Elle struct of column-name → array mappings.
fn elle_struct_to_columns(val: &Value, name: &str) -> Result<Vec<Series>, (SignalBits, Value)> {
    let map = if let Some(m) = val.as_struct() {
        m.iter()
            .filter_map(|(k, v)| {
                if let TableKey::Keyword(s) = k {
                    Some((s.clone(), *v))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>()
    } else {
        return Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected struct of column arrays", name),
            ),
        ));
    };

    let mut columns = Vec::new();
    for (col_name, val) in map {
        let arr = if let Some(a) = val.as_array() {
            a.to_vec()
        } else if let Some(a) = val.as_array_mut() {
            a.borrow().clone()
        } else {
            return Err((
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{}: column '{}' must be an array", name, col_name),
                ),
            ));
        };

        let series = elle_values_to_series(&col_name, &arr, name)?;
        columns.push(series);
    }
    Ok(columns)
}

/// Convert Elle values to a Polars Series, inferring type from first non-nil value.
fn elle_values_to_series(
    col_name: &str,
    values: &[Value],
    prim_name: &str,
) -> Result<Series, (SignalBits, Value)> {
    let first_non_nil = values.iter().find(|v| !v.is_nil());

    match first_non_nil {
        None => Ok(Series::new_null(col_name.into(), values.len())),
        Some(v) if v.as_int().is_some() => {
            let vals: Vec<Option<i64>> = values
                .iter()
                .map(|v| if v.is_nil() { None } else { v.as_int() })
                .collect();
            Ok(Series::new(col_name.into(), &vals))
        }
        Some(v) if v.as_float().is_some() => {
            let vals: Vec<Option<f64>> = values
                .iter()
                .map(|v| if v.is_nil() { None } else { v.as_float() })
                .collect();
            Ok(Series::new(col_name.into(), &vals))
        }
        Some(v) if v.as_bool().is_some() => {
            let vals: Vec<Option<bool>> = values
                .iter()
                .map(|v| if v.is_nil() { None } else { v.as_bool() })
                .collect();
            Ok(Series::new(col_name.into(), &vals))
        }
        Some(v) if v.with_string(|_| ()).is_some() => {
            let vals: Vec<Option<String>> = values
                .iter()
                .map(|v| {
                    if v.is_nil() {
                        None
                    } else {
                        v.with_string(|s| s.to_owned())
                    }
                })
                .collect();
            Ok(Series::new(col_name.into(), &vals))
        }
        _ => Err((
            SIG_ERROR,
            error_val(
                "polars-error",
                format!("{}: cannot infer type for column '{}'", prim_name, col_name),
            ),
        )),
    }
}

// ---------------------------------------------------------------------------
// Primitives — DataFrame construction
// ---------------------------------------------------------------------------

/// (polars/df columns) — create a DataFrame from a struct of column-name → array mappings.
fn prim_df(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/df";
    let columns = match elle_struct_to_columns(&args[0], name) {
        Ok(c) => c,
        Err(e) => return e,
    };

    let columns: Vec<Column> = columns.into_iter().map(Column::from).collect();
    match DataFrame::new(columns) {
        Ok(df) => (SIG_OK, Value::external("polars/df", DfWrap(df))),
        Err(e) => (
            SIG_ERROR,
            error_val("polars-error", format!("{}: {}", name, e)),
        ),
    }
}

/// (polars/read-csv text) — parse CSV text into a DataFrame.
fn prim_read_csv(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/read-csv";
    let text = match extract_string(&args[0], name) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let cursor = Cursor::new(text.into_bytes());
    match CsvReader::new(cursor).finish() {
        Ok(df) => (SIG_OK, Value::external("polars/df", DfWrap(df))),
        Err(e) => (
            SIG_ERROR,
            error_val("polars-error", format!("{}: {}", name, e)),
        ),
    }
}

/// (polars/write-csv df) — serialize a DataFrame to CSV text.
fn prim_write_csv(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/write-csv";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };

    let mut buf = Vec::new();
    let mut df_clone = df.0.clone();
    match CsvWriter::new(&mut buf).finish(&mut df_clone) {
        Ok(_) => match String::from_utf8(buf) {
            Ok(s) => (SIG_OK, Value::string(s)),
            Err(e) => (
                SIG_ERROR,
                error_val("polars-error", format!("{}: {}", name, e)),
            ),
        },
        Err(e) => (
            SIG_ERROR,
            error_val("polars-error", format!("{}: {}", name, e)),
        ),
    }
}

/// (polars/read-parquet bytes) — read Parquet bytes into a DataFrame.
fn prim_read_parquet(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/read-parquet";
    let bytes = match args[0].as_bytes() {
        Some(b) => b.to_vec(),
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{}: expected bytes, got {}", name, args[0].type_name()),
                ),
            )
        }
    };

    let cursor = Cursor::new(bytes);
    match ParquetReader::new(cursor).finish() {
        Ok(df) => (SIG_OK, Value::external("polars/df", DfWrap(df))),
        Err(e) => (
            SIG_ERROR,
            error_val("polars-error", format!("{}: {}", name, e)),
        ),
    }
}

/// (polars/write-parquet df) — serialize a DataFrame to Parquet bytes.
fn prim_write_parquet(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/write-parquet";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };

    let mut buf = Vec::new();
    let mut df_clone = df.0.clone();
    match ParquetWriter::new(&mut buf).finish(&mut df_clone) {
        Ok(_) => (SIG_OK, Value::bytes(buf)),
        Err(e) => (
            SIG_ERROR,
            error_val("polars-error", format!("{}: {}", name, e)),
        ),
    }
}

/// (polars/read-json text) — parse JSON (newline-delimited) into a DataFrame.
fn prim_read_json(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/read-json";
    let text = match extract_string(&args[0], name) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let cursor = Cursor::new(text.into_bytes());
    match JsonReader::new(cursor).finish() {
        Ok(df) => (SIG_OK, Value::external("polars/df", DfWrap(df))),
        Err(e) => (
            SIG_ERROR,
            error_val("polars-error", format!("{}: {}", name, e)),
        ),
    }
}

// ---------------------------------------------------------------------------
// Primitives — DataFrame inspection
// ---------------------------------------------------------------------------

/// (polars/shape df) — return [rows cols] dimensions.
fn prim_shape(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/shape";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let (rows, cols) = df.0.shape();
    (
        SIG_OK,
        Value::array(vec![Value::int(rows as i64), Value::int(cols as i64)]),
    )
}

/// (polars/columns df) — return column names as an array of strings.
fn prim_columns(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/columns";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let names: Vec<Value> =
        df.0.get_column_names()
            .iter()
            .map(|n| Value::string(n.as_str()))
            .collect();
    (SIG_OK, Value::array(names))
}

/// (polars/dtypes df) — return column data types as an array of strings.
fn prim_dtypes(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/dtypes";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let types: Vec<Value> =
        df.0.dtypes()
            .iter()
            .map(|dt| Value::string(format!("{}", dt)))
            .collect();
    (SIG_OK, Value::array(types))
}

/// (polars/head df n) — first n rows (default 5).
fn prim_head(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/head";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let n = if args.len() > 1 {
        args[1].as_int().unwrap_or(5) as usize
    } else {
        5
    };
    let result = df.0.head(Some(n));
    (SIG_OK, Value::external("polars/df", DfWrap(result)))
}

/// (polars/tail df n) — last n rows (default 5).
fn prim_tail(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/tail";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let n = if args.len() > 1 {
        args[1].as_int().unwrap_or(5) as usize
    } else {
        5
    };
    let result = df.0.tail(Some(n));
    (SIG_OK, Value::external("polars/df", DfWrap(result)))
}

/// (polars/display df) — pretty-print a DataFrame as a string.
fn prim_display(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/display";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    (SIG_OK, Value::string(format!("{}", df.0)))
}

/// (polars/to-rows df) — convert a DataFrame to an Elle array of structs.
fn prim_to_rows(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/to-rows";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    (SIG_OK, df_to_elle(&df.0))
}

/// (polars/column df col-name) — extract a single column as an Elle array.
fn prim_column(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/column";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let col_name = match extract_string(&args[1], name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match df.0.column(&col_name) {
        Ok(s) => (
            SIG_OK,
            Value::array(series_to_elle(s.as_materialized_series())),
        ),
        Err(e) => (
            SIG_ERROR,
            error_val("polars-error", format!("{}: {}", name, e)),
        ),
    }
}

// ---------------------------------------------------------------------------
// Primitives — DataFrame operations
// ---------------------------------------------------------------------------

/// (polars/select df columns) — select columns by name.
fn prim_select(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/select";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let cols = match extract_string_list(&args[1], name) {
        Ok(c) => c,
        Err(e) => return e,
    };
    match df.0.select(&cols) {
        Ok(result) => (SIG_OK, Value::external("polars/df", DfWrap(result))),
        Err(e) => (
            SIG_ERROR,
            error_val("polars-error", format!("{}: {}", name, e)),
        ),
    }
}

/// (polars/drop df columns) — drop columns by name.
fn prim_drop(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/drop";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let cols = match extract_string_list(&args[1], name) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let result = df.0.drop_many(&cols);
    (SIG_OK, Value::external("polars/df", DfWrap(result)))
}

/// (polars/rename df from to) — rename a column.
fn prim_rename(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/rename";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let from = match extract_string(&args[1], name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let to = match extract_string(&args[2], name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut result = df.0.clone();
    match result.rename(&from, PlSmallStr::from(to.as_str())) {
        Ok(_) => (SIG_OK, Value::external("polars/df", DfWrap(result))),
        Err(e) => (
            SIG_ERROR,
            error_val("polars-error", format!("{}: {}", name, e)),
        ),
    }
}

/// (polars/slice df offset length) — take a slice of rows.
fn prim_slice(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/slice";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let offset = args[1].as_int().unwrap_or(0);
    let length = args[2].as_int().unwrap_or(0) as usize;
    let result = df.0.slice(offset, length);
    (SIG_OK, Value::external("polars/df", DfWrap(result)))
}

/// (polars/sample df n) — random sample of n rows.
fn prim_sample(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/sample";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let n = args[1].as_int().unwrap_or(1) as usize;
    match df.0.sample_n_literal(n, false, false, None) {
        Ok(result) => (SIG_OK, Value::external("polars/df", DfWrap(result))),
        Err(e) => (
            SIG_ERROR,
            error_val("polars-error", format!("{}: {}", name, e)),
        ),
    }
}

/// (polars/sort df column) or (polars/sort df column :desc)
fn prim_sort(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/sort";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let col = match extract_string(&args[1], name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let descending = if args.len() > 2 {
        // Check if third arg is :desc keyword
        args[2].with_string(|s| s == "desc").unwrap_or(false)
    } else {
        false
    };
    match df.0.sort(
        [col.as_str()],
        SortMultipleOptions::new().with_order_descending(descending),
    ) {
        Ok(result) => (SIG_OK, Value::external("polars/df", DfWrap(result))),
        Err(e) => (
            SIG_ERROR,
            error_val("polars-error", format!("{}: {}", name, e)),
        ),
    }
}

/// (polars/unique df columns) — unique rows by subset of columns.
fn prim_unique(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/unique";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let cols = if args.len() > 1 {
        match extract_string_list(&args[1], name) {
            Ok(c) => Some(c),
            Err(e) => return e,
        }
    } else {
        None
    };

    let result = match cols {
        Some(ref c) => {
            df.0.unique::<&[String], String>(Some(c.as_slice()), UniqueKeepStrategy::First, None)
        }
        None => {
            df.0.unique::<&[String], String>(None, UniqueKeepStrategy::First, None)
        }
    };
    match result {
        Ok(r) => (SIG_OK, Value::external("polars/df", DfWrap(r))),
        Err(e) => (
            SIG_ERROR,
            error_val("polars-error", format!("{}: {}", name, e)),
        ),
    }
}

/// (polars/vstack df1 df2) — vertically concatenate two DataFrames.
fn prim_vstack(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/vstack";
    let df1 = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let df2 = match get_df(&args[1], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    match df1.0.vstack(&df2.0) {
        Ok(stacked) => (SIG_OK, Value::external("polars/df", DfWrap(stacked))),
        Err(e) => (
            SIG_ERROR,
            error_val("polars-error", format!("{}: {}", name, e)),
        ),
    }
}

/// (polars/hstack df1 df2) — horizontally concatenate (add columns from df2 to df1).
fn prim_hstack(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/hstack";
    let df1 = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let df2 = match get_df(&args[1], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let cols: Vec<Column> = df2.0.get_columns().to_vec();
    match df1.0.hstack(&cols) {
        Ok(result) => (SIG_OK, Value::external("polars/df", DfWrap(result))),
        Err(e) => (
            SIG_ERROR,
            error_val("polars-error", format!("{}: {}", name, e)),
        ),
    }
}

// ---------------------------------------------------------------------------
// Primitives — Lazy API
// ---------------------------------------------------------------------------

/// (polars/lazy df) — convert DataFrame to LazyFrame.
fn prim_lazy(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/lazy";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let lazy = df.0.clone().lazy();
    (SIG_OK, Value::external("polars/lazy", LazyWrap(lazy)))
}

/// (polars/collect lazy) — execute a LazyFrame, returning a DataFrame.
fn prim_collect(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/collect";
    let lazy = match get_lazy(&args[0], name) {
        Ok(l) => l,
        Err(e) => return e,
    };
    match lazy.0.clone().collect() {
        Ok(df) => (SIG_OK, Value::external("polars/df", DfWrap(df))),
        Err(e) => (
            SIG_ERROR,
            error_val("polars-error", format!("{}: {}", name, e)),
        ),
    }
}

/// (polars/lselect lazy columns) — lazy select columns by name.
fn prim_lselect(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/lselect";
    let lazy = match get_lazy(&args[0], name) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let cols = match extract_string_list(&args[1], name) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let exprs: Vec<Expr> = cols.iter().map(|c| col(c.as_str())).collect();
    let result = lazy.0.clone().select(exprs);
    (SIG_OK, Value::external("polars/lazy", LazyWrap(result)))
}

/// (polars/lfilter lazy col op value) — lazy filter: col op value.
/// op is one of: "=" "!=" "<" ">" "<=" ">="
fn prim_lfilter(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/lfilter";
    let lazy = match get_lazy(&args[0], name) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let col_name = match extract_string(&args[1], name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let op = match extract_string(&args[2], name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let val = &args[3];

    let column = col(col_name.as_str());
    let predicate = if let Some(i) = val.as_int() {
        let lit_val = lit(i);
        match op.as_str() {
            "=" | "==" => column.eq(lit_val),
            "!=" => column.neq(lit_val),
            "<" => column.lt(lit_val),
            ">" => column.gt(lit_val),
            "<=" => column.lt_eq(lit_val),
            ">=" => column.gt_eq(lit_val),
            _ => {
                return (
                    SIG_ERROR,
                    error_val("polars-error", format!("{}: unknown op '{}'", name, op)),
                )
            }
        }
    } else if let Some(f) = val.as_float() {
        let lit_val = lit(f);
        match op.as_str() {
            "=" | "==" => column.eq(lit_val),
            "!=" => column.neq(lit_val),
            "<" => column.lt(lit_val),
            ">" => column.gt(lit_val),
            "<=" => column.lt_eq(lit_val),
            ">=" => column.gt_eq(lit_val),
            _ => {
                return (
                    SIG_ERROR,
                    error_val("polars-error", format!("{}: unknown op '{}'", name, op)),
                )
            }
        }
    } else if let Some(s) = val.with_string(|s| s.to_owned()) {
        let lit_val = lit(s);
        match op.as_str() {
            "=" | "==" => column.eq(lit_val),
            "!=" => column.neq(lit_val),
            "<" => column.lt(lit_val),
            ">" => column.gt(lit_val),
            "<=" => column.lt_eq(lit_val),
            ">=" => column.gt_eq(lit_val),
            _ => {
                return (
                    SIG_ERROR,
                    error_val("polars-error", format!("{}: unknown op '{}'", name, op)),
                )
            }
        }
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: unsupported filter value type", name),
            ),
        );
    };

    let result = lazy.0.clone().filter(predicate);
    (SIG_OK, Value::external("polars/lazy", LazyWrap(result)))
}

/// (polars/lsort lazy column) or (polars/lsort lazy column :desc)
fn prim_lsort(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/lsort";
    let lazy = match get_lazy(&args[0], name) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let col_name = match extract_string(&args[1], name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let descending = if args.len() > 2 {
        args[2].with_string(|s| s == "desc").unwrap_or(false)
    } else {
        false
    };
    let result = lazy.0.clone().sort(
        [col_name.as_str()],
        SortMultipleOptions::new().with_order_descending(descending),
    );
    (SIG_OK, Value::external("polars/lazy", LazyWrap(result)))
}

/// (polars/lgroupby lazy columns aggs)
/// aggs is a struct of output-col → {:col "src" :agg "sum"|"mean"|"min"|"max"|"count"|"first"|"last"}
fn prim_lgroupby(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/lgroupby";
    let lazy = match get_lazy(&args[0], name) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let group_cols = match extract_string_list(&args[1], name) {
        Ok(c) => c,
        Err(e) => return e,
    };

    // Parse aggregation specs
    let agg_map: &[(TableKey, Value)] = if let Some(m) = args[2].as_struct() {
        m
    } else {
        return (
            SIG_ERROR,
            error_val("type-error", format!("{}: aggs must be a struct", name)),
        );
    };

    let mut agg_exprs: Vec<Expr> = Vec::new();
    for (key, spec) in agg_map.iter() {
        let out_name = if let TableKey::Keyword(s) = key {
            s.clone()
        } else {
            continue;
        };

        let spec_map: &[(TableKey, Value)] = if let Some(m) = spec.as_struct() {
            m
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{}: each agg spec must be a struct", name),
                ),
            );
        };

        let src_col = elle::value::sorted_struct_get(spec_map, &TableKey::Keyword("col".into()))
            .and_then(|v| v.with_string(|s| s.to_owned()))
            .ok_or_else(|| {
                (
                    SIG_ERROR,
                    error_val("polars-error", format!("{}: agg spec missing :col", name)),
                )
            });
        let src_col = match src_col {
            Ok(s) => s,
            Err(e) => return e,
        };

        let agg_fn = elle::value::sorted_struct_get(spec_map, &TableKey::Keyword("agg".into()))
            .and_then(|v| v.with_string(|s| s.to_owned()))
            .ok_or_else(|| {
                (
                    SIG_ERROR,
                    error_val("polars-error", format!("{}: agg spec missing :agg", name)),
                )
            });
        let agg_fn = match agg_fn {
            Ok(s) => s,
            Err(e) => return e,
        };

        let expr = match agg_fn.as_str() {
            "sum" => col(src_col.as_str()).sum().alias(out_name.as_str()),
            "mean" => col(src_col.as_str()).mean().alias(out_name.as_str()),
            "min" => col(src_col.as_str()).min().alias(out_name.as_str()),
            "max" => col(src_col.as_str()).max().alias(out_name.as_str()),
            "count" => col(src_col.as_str()).count().alias(out_name.as_str()),
            "first" => col(src_col.as_str()).first().alias(out_name.as_str()),
            "last" => col(src_col.as_str()).last().alias(out_name.as_str()),
            other => {
                return (
                    SIG_ERROR,
                    error_val(
                        "polars-error",
                        format!("{}: unknown agg function '{}'", name, other),
                    ),
                )
            }
        };
        agg_exprs.push(expr);
    }

    let group_exprs: Vec<Expr> = group_cols.iter().map(|c| col(c.as_str())).collect();
    let result = lazy.0.clone().group_by(group_exprs).agg(agg_exprs);
    (SIG_OK, Value::external("polars/lazy", LazyWrap(result)))
}

/// (polars/ljoin left right on how)
/// how is "inner", "left", "outer", or "cross"
fn prim_ljoin(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/ljoin";
    let left = match get_lazy(&args[0], name) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let right = match get_lazy(&args[1], name) {
        Ok(l) => l,
        Err(e) => return e,
    };
    let on_cols = match extract_string_list(&args[2], name) {
        Ok(c) => c,
        Err(e) => return e,
    };
    let how_str = if args.len() > 3 {
        match extract_string(&args[3], name) {
            Ok(s) => s,
            Err(e) => return e,
        }
    } else {
        "inner".into()
    };

    let how = match how_str.as_str() {
        "inner" => JoinType::Inner,
        "left" => JoinType::Left,
        "full" | "outer" => JoinType::Full,
        "cross" => JoinType::Cross,
        other => {
            return (
                SIG_ERROR,
                error_val(
                    "polars-error",
                    format!("{}: unknown join type '{}'", name, other),
                ),
            )
        }
    };

    let on_exprs: Vec<Expr> = on_cols.iter().map(|c| col(c.as_str())).collect();
    let result = left.0.clone().join(
        right.0.clone(),
        on_exprs.clone(),
        on_exprs,
        JoinArgs::new(how),
    );
    (SIG_OK, Value::external("polars/lazy", LazyWrap(result)))
}

// ---------------------------------------------------------------------------
// Primitives — Describe / stats
// ---------------------------------------------------------------------------

/// (polars/describe df) — summary statistics (like pandas describe).
fn prim_describe(args: &[Value]) -> (SignalBits, Value) {
    let name = "polars/describe";
    let df = match get_df(&args[0], name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    // Build basic stats manually since describe() requires extra feature
    let mut stat_rows: Vec<Value> = Vec::new();
    for col in df.0.get_columns() {
        let s = col.as_materialized_series();
        let mut fields = BTreeMap::new();
        fields.insert(
            TableKey::Keyword("column".into()),
            Value::string(s.name().as_str()),
        );
        fields.insert(
            TableKey::Keyword("dtype".into()),
            Value::string(format!("{}", s.dtype())),
        );
        fields.insert(
            TableKey::Keyword("count".into()),
            Value::int(s.len() as i64),
        );
        fields.insert(
            TableKey::Keyword("null_count".into()),
            Value::int(s.null_count() as i64),
        );
        stat_rows.push(Value::struct_from(fields));
    }
    (SIG_OK, Value::array(stat_rows))
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    // Construction / IO
    PrimitiveDef {
        name: "polars/df",
        func: prim_df,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Create a DataFrame from a struct of column-name → array mappings.",
        params: &["columns"],
        category: "polars",
        example: r#"(polars/df {:name ["Alice" "Bob"] :age [30 25]})"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/read-csv",
        func: prim_read_csv,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse CSV text into a DataFrame.",
        params: &["text"],
        category: "polars",
        example: r#"(polars/read-csv "name,age\nAlice,30")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/write-csv",
        func: prim_write_csv,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Serialize a DataFrame to CSV text.",
        params: &["df"],
        category: "polars",
        example: "(polars/write-csv my-df)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/read-parquet",
        func: prim_read_parquet,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Read Parquet bytes into a DataFrame.",
        params: &["bytes"],
        category: "polars",
        example: "(polars/read-parquet pq-bytes)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/write-parquet",
        func: prim_write_parquet,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Serialize a DataFrame to Parquet bytes.",
        params: &["df"],
        category: "polars",
        example: "(polars/write-parquet my-df)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/read-json",
        func: prim_read_json,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Parse JSON text into a DataFrame.",
        params: &["text"],
        category: "polars",
        example: r#"(polars/read-json "[{\"a\":1},{\"a\":2}]")"#,
        aliases: &[],
    },
    // Inspection
    PrimitiveDef {
        name: "polars/shape",
        func: prim_shape,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return [rows cols] dimensions of a DataFrame.",
        params: &["df"],
        category: "polars",
        example: "(polars/shape my-df)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/columns",
        func: prim_columns,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return column names as an array of strings.",
        params: &["df"],
        category: "polars",
        example: "(polars/columns my-df)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/dtypes",
        func: prim_dtypes,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return column data types as an array of strings.",
        params: &["df"],
        category: "polars",
        example: "(polars/dtypes my-df)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/head",
        func: prim_head,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Return first n rows (default 5).",
        params: &["df", "n"],
        category: "polars",
        example: "(polars/head my-df 10)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/tail",
        func: prim_tail,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Return last n rows (default 5).",
        params: &["df", "n"],
        category: "polars",
        example: "(polars/tail my-df 10)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/display",
        func: prim_display,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Pretty-print a DataFrame as a formatted table string.",
        params: &["df"],
        category: "polars",
        example: "(polars/display my-df)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/to-rows",
        func: prim_to_rows,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert a DataFrame to an Elle array of structs (one struct per row).",
        params: &["df"],
        category: "polars",
        example: "(polars/to-rows my-df)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/column",
        func: prim_column,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Extract a single column as an Elle array.",
        params: &["df", "column-name"],
        category: "polars",
        example: r#"(polars/column my-df "name")"#,
        aliases: &[],
    },
    // Operations
    PrimitiveDef {
        name: "polars/select",
        func: prim_select,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Select columns by name (array of strings).",
        params: &["df", "columns"],
        category: "polars",
        example: r#"(polars/select my-df ["name" "age"])"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/drop",
        func: prim_drop,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Drop columns by name (array of strings).",
        params: &["df", "columns"],
        category: "polars",
        example: r#"(polars/drop my-df ["temp"])"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/rename",
        func: prim_rename,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Rename a column: (polars/rename df old-name new-name).",
        params: &["df", "from", "to"],
        category: "polars",
        example: r#"(polars/rename my-df "old" "new")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/slice",
        func: prim_slice,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Take a slice of rows: (polars/slice df offset length).",
        params: &["df", "offset", "length"],
        category: "polars",
        example: "(polars/slice my-df 0 10)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/sample",
        func: prim_sample,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Random sample of n rows.",
        params: &["df", "n"],
        category: "polars",
        example: "(polars/sample my-df 5)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/sort",
        func: prim_sort,
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: r#"Sort by column. Optional third arg "desc" for descending."#,
        params: &["df", "column", "order"],
        category: "polars",
        example: r#"(polars/sort my-df "age" "desc")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/unique",
        func: prim_unique,
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Unique rows, optionally by subset of columns.",
        params: &["df", "columns"],
        category: "polars",
        example: r#"(polars/unique my-df ["name"])"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/vstack",
        func: prim_vstack,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Vertically concatenate two DataFrames (same schema).",
        params: &["df1", "df2"],
        category: "polars",
        example: "(polars/vstack df1 df2)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/hstack",
        func: prim_hstack,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Horizontally concatenate (add columns from df2 to df1).",
        params: &["df1", "df2"],
        category: "polars",
        example: "(polars/hstack df1 df2)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/describe",
        func: prim_describe,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Summary statistics for all numeric columns.",
        params: &["df"],
        category: "polars",
        example: "(polars/describe my-df)",
        aliases: &[],
    },
    // Lazy API
    PrimitiveDef {
        name: "polars/lazy",
        func: prim_lazy,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert a DataFrame to a LazyFrame for deferred evaluation.",
        params: &["df"],
        category: "polars",
        example: "(polars/lazy my-df)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/collect",
        func: prim_collect,
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Execute a LazyFrame query, returning a DataFrame.",
        params: &["lazy"],
        category: "polars",
        example: "(polars/collect my-lazy)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/lselect",
        func: prim_lselect,
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Lazy select columns by name.",
        params: &["lazy", "columns"],
        category: "polars",
        example: r#"(polars/lselect my-lazy ["name" "age"])"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/lfilter",
        func: prim_lfilter,
        signal: Signal::errors(),
        arity: Arity::Exact(4),
        doc: r#"Lazy filter: (polars/lfilter lazy col op value). Op is "=", "!=", "<", ">", "<=", ">="."#,
        params: &["lazy", "column", "op", "value"],
        category: "polars",
        example: r#"(polars/lfilter my-lazy "age" ">" 25)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/lsort",
        func: prim_lsort,
        signal: Signal::errors(),
        arity: Arity::Range(2, 3),
        doc: r#"Lazy sort by column. Optional third arg "desc" for descending."#,
        params: &["lazy", "column", "order"],
        category: "polars",
        example: r#"(polars/lsort my-lazy "age" "desc")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/lgroupby",
        func: prim_lgroupby,
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: r#"Lazy group-by with aggregations. Aggs is a struct: {:out-name {:col "src" :agg "sum"|"mean"|"min"|"max"|"count"|"first"|"last"}}."#,
        params: &["lazy", "group-columns", "aggs"],
        category: "polars",
        example: r#"(polars/lgroupby my-lazy ["dept"] {:total {:col "salary" :agg "sum"}})"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "polars/ljoin",
        func: prim_ljoin,
        signal: Signal::errors(),
        arity: Arity::Range(3, 4),
        doc: r#"Lazy join: (polars/ljoin left right on-cols how). How is "inner", "left", "full", "cross". Default "inner"."#,
        params: &["left", "right", "on-columns", "how"],
        category: "polars",
        example: r#"(polars/ljoin l r ["id"] "left")"#,
        aliases: &[],
    },
];
elle::elle_plugin_init!(PRIMITIVES, "polars/");
