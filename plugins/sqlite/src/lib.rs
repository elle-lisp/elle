//! Elle SQLite plugin — database access via the `rusqlite` crate.

use elle::effects::Effect;
use elle::plugin::PluginContext;
use elle::primitives::def::PrimitiveDef;
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::types::Arity;
use elle::value::{error_val, TableKey, Value};
use rusqlite::types::ValueRef;
use rusqlite::Connection;
use std::cell::RefCell;
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
        let short_name = def.name.strip_prefix("sqlite/").unwrap_or(def.name);
        fields.insert(
            TableKey::Keyword(short_name.into()),
            Value::native_fn(def.func),
        );
    }
    Value::struct_from(fields)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Extract the `RefCell<Connection>` from an external value, or return an error.
fn get_db<'a>(
    args: &'a [Value],
    name: &str,
) -> Result<&'a RefCell<Connection>, (SignalBits, Value)> {
    args[0].as_external::<RefCell<Connection>>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected sqlite/db, got {}", name, args[0].type_name()),
            ),
        )
    })
}

/// Extract a SQL string from args[1].
fn get_sql(args: &[Value], name: &str) -> Result<String, (SignalBits, Value)> {
    args[1].with_string(|s| s.to_string()).ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected string, got {}", name, args[1].type_name()),
            ),
        )
    })
}

/// Convert an Elle value to a boxed SQL parameter.
fn value_to_sql(
    v: Value,
    name: &str,
) -> Result<Box<dyn rusqlite::types::ToSql>, (SignalBits, Value)> {
    if v.is_nil() {
        Ok(Box::new(rusqlite::types::Null))
    } else if let Some(b) = v.as_bool() {
        Ok(Box::new(b))
    } else if let Some(i) = v.as_int() {
        Ok(Box::new(i))
    } else if let Some(f) = v.as_float() {
        Ok(Box::new(f))
    } else if let Some(s) = v.with_string(|s| s.to_string()) {
        Ok(Box::new(s))
    } else {
        Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: unsupported param type {}", name, v.type_name()),
            ),
        ))
    }
}

/// Extract optional params from args[2] (a list or tuple) into boxed SQL params.
fn extract_params(
    args: &[Value],
    name: &str,
) -> Result<Vec<Box<dyn rusqlite::types::ToSql>>, (SignalBits, Value)> {
    if args.len() < 3 {
        return Ok(vec![]);
    }
    let pval = args[2];
    // Try tuple first (slice access)
    if let Some(elems) = pval.as_tuple() {
        return elems.iter().map(|v| value_to_sql(*v, name)).collect();
    }
    // Try array
    if let Some(arr) = pval.as_array() {
        let arr = arr.borrow();
        return arr.iter().map(|v| value_to_sql(*v, name)).collect();
    }
    // Try proper list (cons cells terminated by empty list)
    if pval.is_empty_list() {
        return Ok(vec![]);
    }
    if pval.as_cons().is_some() {
        match pval.list_to_vec() {
            Ok(vec) => return vec.iter().map(|v| value_to_sql(*v, name)).collect(),
            Err(_) => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!("{}: params must be a list, tuple, or array", name),
                    ),
                ));
            }
        }
    }
    Err((
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "{}: expected list/tuple/array for params, got {}",
                name,
                pval.type_name()
            ),
        ),
    ))
}

/// Map a rusqlite error to an Elle error signal.
fn sql_err(name: &str, e: rusqlite::Error) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val("sqlite-error", format!("{}: {}", name, e)),
    )
}

// ---------------------------------------------------------------------------
// Primitives
// ---------------------------------------------------------------------------

fn prim_sqlite_open(args: &[Value]) -> (SignalBits, Value) {
    let path = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("sqlite/open: expected string, got {}", args[0].type_name()),
                ),
            );
        }
    };
    let conn = if path == ":memory:" {
        Connection::open_in_memory()
    } else {
        Connection::open(&path)
    };
    match conn {
        Ok(c) => (SIG_OK, Value::external("sqlite/db", RefCell::new(c))),
        Err(e) => sql_err("sqlite/open", e),
    }
}

fn prim_sqlite_close(_args: &[Value]) -> (SignalBits, Value) {
    // Connection closes when the Rc'd external object is garbage collected.
    // This is intentionally a no-op.
    (SIG_OK, Value::NIL)
}

fn prim_sqlite_execute(args: &[Value]) -> (SignalBits, Value) {
    let name = "sqlite/execute";
    let db = match get_db(args, name) {
        Ok(db) => db,
        Err(e) => return e,
    };
    let sql = match get_sql(args, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let params = match extract_params(args, name) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let conn = db.borrow();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    match conn.execute(&sql, param_refs.as_slice()) {
        Ok(n) => (SIG_OK, Value::int(n as i64)),
        Err(e) => sql_err(name, e),
    }
}

fn prim_sqlite_query(args: &[Value]) -> (SignalBits, Value) {
    let name = "sqlite/query";
    let db = match get_db(args, name) {
        Ok(db) => db,
        Err(e) => return e,
    };
    let sql = match get_sql(args, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let params = match extract_params(args, name) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let conn = db.borrow();
    let mut stmt: rusqlite::Statement<'_> = match conn.prepare(&sql) {
        Ok(s) => s,
        Err(e) => return sql_err(name, e),
    };
    let column_names: Vec<String> = stmt
        .column_names()
        .iter()
        .map(|s: &&str| s.to_string())
        .collect();
    let param_refs: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let mut query_rows: rusqlite::Rows<'_> = match stmt.query(param_refs.as_slice()) {
        Ok(r) => r,
        Err(e) => return sql_err(name, e),
    };
    let mut rows = Vec::new();
    loop {
        match query_rows.next() {
            Ok(Some(row)) => {
                let mut fields = BTreeMap::new();
                for (i, col_name) in column_names.iter().enumerate() {
                    let val_ref: ValueRef<'_> = match row.get_ref(i) {
                        Ok(v) => v,
                        Err(e) => return sql_err(name, e),
                    };
                    let value = match val_ref {
                        ValueRef::Null => Value::NIL,
                        ValueRef::Integer(n) => Value::int(n),
                        ValueRef::Real(f) => Value::float(f),
                        ValueRef::Text(s) => Value::string(std::str::from_utf8(s).unwrap_or("")),
                        ValueRef::Blob(b) => Value::string(format!("<blob {} bytes>", b.len())),
                    };
                    fields.insert(TableKey::Keyword(col_name.clone()), value);
                }
                rows.push(Value::struct_from(fields));
            }
            Ok(None) => break,
            Err(e) => return sql_err(name, e),
        }
    }
    (SIG_OK, elle::list(rows))
}

// ---------------------------------------------------------------------------
// Registration table
// ---------------------------------------------------------------------------

static PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "sqlite/open",
        func: prim_sqlite_open,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Open a SQLite database. Use \":memory:\" for in-memory.",
        params: &["path"],
        category: "sqlite",
        example: r#"(sqlite/open ":memory:")"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "sqlite/close",
        func: prim_sqlite_close,
        effect: Effect::pure(),
        arity: Arity::Exact(1),
        doc: "Close a SQLite database (no-op; connection closes on GC).",
        params: &["db"],
        category: "sqlite",
        example: r#"(sqlite/close db)"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "sqlite/execute",
        func: prim_sqlite_execute,
        effect: Effect::raises(),
        arity: Arity::Range(2, 3),
        doc: "Execute SQL that doesn't return rows. Returns rows affected.",
        params: &["db", "sql", "params?"],
        category: "sqlite",
        example: r#"(sqlite/execute db "INSERT INTO t VALUES (?1)" (list 42))"#,
        aliases: &[],
    },
    PrimitiveDef {
        name: "sqlite/query",
        func: prim_sqlite_query,
        effect: Effect::raises(),
        arity: Arity::Range(2, 3),
        doc: "Execute a query and return results as a list of structs.",
        params: &["db", "sql", "params?"],
        category: "sqlite",
        example: r#"(sqlite/query db "SELECT * FROM t")"#,
        aliases: &[],
    },
];
