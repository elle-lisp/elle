//! Schema loading: parse `.proto` text or binary `FileDescriptorSet` into a
//! `prost_reflect::DescriptorPool`.

use std::io::Write;

use prost_reflect::DescriptorPool;
use protobuf::Message as ProtobufMessage; // needed for FileDescriptorSet::write_to_bytes

use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::{error_val, TableKey, Value};

// ---------------------------------------------------------------------------
// Internal: parse .proto text via temp file
// ---------------------------------------------------------------------------

/// Parse a `.proto` source string into a `DescriptorPool`.
///
/// `virtual_name` is the filename used for import resolution context
/// (e.g. `"input.proto"`). `include_dirs` are additional directories
/// searched when resolving `import` statements.
pub(crate) fn parse_proto_string(
    proto_src: &str,
    virtual_name: &str,
    include_dirs: &[String],
) -> Result<DescriptorPool, String> {
    // protobuf-parse has no in-memory string API; it requires files on disk.
    let dir = tempfile::tempdir().map_err(|e| format!("failed to create temp dir: {}", e))?;
    let proto_path = dir.path().join(virtual_name);

    {
        let mut f = std::fs::File::create(&proto_path)
            .map_err(|e| format!("failed to create temp file: {}", e))?;
        f.write_all(proto_src.as_bytes())
            .map_err(|e| format!("failed to write temp file: {}", e))?;
    }

    // protobuf_parse::Parser builder methods return &mut Parser.
    // Call each for its side effect, then call parse_and_typecheck.
    let mut parser = protobuf_parse::Parser::new();
    parser.pure();
    parser.include(dir.path());
    parser.input(&proto_path);

    for extra_dir in include_dirs {
        parser.include(extra_dir);
    }

    // Use parse_and_typecheck() (not file_descriptor_set()) to include ALL
    // files (input + transitive dependencies). file_descriptor_set() strips
    // dependency files from the result, breaking cross-file type resolution.
    let parsed = parser.parse_and_typecheck().map_err(|e| format!("{}", e))?;

    let mut fds = protobuf::descriptor::FileDescriptorSet::new();
    fds.file = parsed.file_descriptors;

    // Serialization bridge: protobuf crate → bytes → prost-reflect.
    // This is a one-time cost at schema load time.
    let bytes = fds
        .write_to_bytes()
        .map_err(|e| format!("failed to serialize FileDescriptorSet: {}", e))?;

    DescriptorPool::decode(bytes.as_slice())
        .map_err(|e| format!("failed to decode FileDescriptorSet: {}", e))
}

// ---------------------------------------------------------------------------
// Primitive: protobuf/schema
// ---------------------------------------------------------------------------

/// `(protobuf/schema proto-string)`
/// `(protobuf/schema proto-string {:path "foo.proto" :includes ["dir1"]})`
///
/// Parse `.proto` source text into a descriptor pool.
pub fn prim_schema(args: &[Value]) -> (SignalBits, Value) {
    const PRIM: &str = "protobuf/schema";

    // args[0]: proto source string
    let proto_src = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("{}: expected string, got {}", PRIM, args[0].type_name()),
                ),
            );
        }
    };

    // Default options
    let mut virtual_name = "input.proto".to_string();
    let mut include_dirs: Vec<String> = Vec::new();

    // args[1]: optional options struct {:path :includes}
    if args.len() >= 2 && !args[1].is_nil() {
        let opts = args[1];
        // Verify it's a struct (immutable or mutable)
        let is_struct = opts.as_struct().is_some() || opts.as_struct_mut().is_some();
        if !is_struct {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "{}: expected struct for options, got {}",
                        PRIM,
                        opts.type_name()
                    ),
                ),
            );
        }

        // Extract :path
        let path_key = TableKey::Keyword("path".into());
        if let Some(path_val) = struct_get(opts, &path_key) {
            match path_val.with_string(|s| s.to_string()) {
                Some(p) => virtual_name = p,
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "{}: :path must be a string, got {}",
                                PRIM,
                                path_val.type_name()
                            ),
                        ),
                    );
                }
            }
        }

        // Extract :includes (array of strings)
        let includes_key = TableKey::Keyword("includes".into());
        if let Some(includes_val) = struct_get(opts, &includes_key) {
            match extract_string_array(includes_val, PRIM, ":includes") {
                Ok(dirs) => include_dirs = dirs,
                Err(e) => return e,
            }
        }
    }

    match parse_proto_string(&proto_src, &virtual_name, &include_dirs) {
        Ok(pool) => (SIG_OK, Value::external("protobuf/pool", pool)),
        Err(e) => (
            SIG_ERROR,
            error_val("protobuf-error", format!("{}: {}", PRIM, e)),
        ),
    }
}

// ---------------------------------------------------------------------------
// Primitive: protobuf/schema-bytes
// ---------------------------------------------------------------------------

/// `(protobuf/schema-bytes fds-bytes)`
///
/// Load a pre-compiled binary `FileDescriptorSet` into a descriptor pool.
pub fn prim_schema_bytes(args: &[Value]) -> (SignalBits, Value) {
    const PRIM: &str = "protobuf/schema-bytes";

    let bytes = match extract_bytes(args[0], PRIM) {
        Ok(b) => b,
        Err(e) => return e,
    };

    match DescriptorPool::decode(bytes.as_slice()) {
        Ok(pool) => (SIG_OK, Value::external("protobuf/pool", pool)),
        Err(e) => (
            SIG_ERROR,
            error_val("protobuf-error", format!("{}: {}", PRIM, e)),
        ),
    }
}

// ---------------------------------------------------------------------------
// Shared helpers (pub(crate) for use in other modules)
// ---------------------------------------------------------------------------

/// Extract a `DescriptorPool` reference from a Value, or return a type-error.
pub(crate) fn get_pool<'a>(
    val: &'a Value,
    prim: &str,
) -> Result<&'a DescriptorPool, (SignalBits, Value)> {
    val.as_external::<DescriptorPool>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected protobuf/pool, got {}", prim, val.type_name()),
            ),
        )
    })
}

/// Extract bytes (immutable or mutable) from a Value into an owned Vec.
pub(crate) fn extract_bytes(val: Value, prim: &str) -> Result<Vec<u8>, (SignalBits, Value)> {
    if let Some(b) = val.as_bytes() {
        return Ok(b.to_vec());
    }
    if let Some(b) = val.as_bytes_mut() {
        return Ok(b.borrow().to_vec());
    }
    Err((
        SIG_ERROR,
        error_val(
            "type-error",
            format!("{}: expected bytes, got {}", prim, val.type_name()),
        ),
    ))
}

/// Extract an array of strings from a Value.
pub(crate) fn extract_string_array(
    val: Value,
    prim: &str,
    field: &str,
) -> Result<Vec<String>, (SignalBits, Value)> {
    let items: Vec<Value> = if let Some(arr) = val.as_array() {
        arr.to_vec()
    } else if let Some(arr) = val.as_array_mut() {
        arr.borrow().to_vec()
    } else {
        return Err((
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: {} must be an array, got {}",
                    prim,
                    field,
                    val.type_name()
                ),
            ),
        ));
    };

    let mut result = Vec::with_capacity(items.len());
    for item in items {
        match item.with_string(|s| s.to_string()) {
            Some(s) => result.push(s),
            None => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "{}: {} elements must be strings, got {}",
                            prim,
                            field,
                            item.type_name()
                        ),
                    ),
                ));
            }
        }
    }
    Ok(result)
}

/// Look up a key in a struct (immutable or mutable).
pub(crate) fn struct_get(val: Value, key: &TableKey) -> Option<Value> {
    if let Some(s) = val.as_struct() {
        return s.get(key).copied();
    }
    if let Some(s) = val.as_struct_mut() {
        return s.borrow().get(key).copied();
    }
    None
}

/// Iterate over keyword key-value pairs in a struct (immutable or mutable).
/// Calls `f(keyword_name, value)` for each keyword key entry.
pub(crate) fn struct_keyword_iter(val: Value, mut f: impl FnMut(&str, Value)) {
    let map: Vec<(TableKey, Value)> = if let Some(s) = val.as_struct() {
        s.iter().map(|(k, v)| (k.clone(), *v)).collect()
    } else if let Some(s) = val.as_struct_mut() {
        s.borrow().iter().map(|(k, v)| (k.clone(), *v)).collect()
    } else {
        return;
    };

    for (key, val) in map {
        if let TableKey::Keyword(name) = key {
            f(&name, val);
        }
    }
}
