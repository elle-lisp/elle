//! Path manipulation primitives.
//!
//! Thin wrappers around `crate::path`. No camino imports here.

use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Call `f` with the string content of `val`, or return a type error
/// tagged with `prim_name`.
fn with_str_arg<F>(val: &Value, prim_name: &str, f: F) -> (SignalBits, Value)
where
    F: FnOnce(&str) -> (SignalBits, Value),
{
    if let Some(result) = val.with_string(|s| f(s)) {
        result
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected string, got {}", prim_name, val.type_name()),
            ),
        )
    }
}

fn prim_path_join(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                "path/join: expected at least 1 argument, got 0",
            ),
        );
    }
    let mut parts = Vec::with_capacity(args.len());
    for arg in args {
        if let Some(s) = arg.with_string(|s| s.to_string()) {
            parts.push(s);
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("path/join: expected string, got {}", arg.type_name()),
                ),
            );
        }
    }
    let refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
    (SIG_OK, Value::string(crate::path::join(&refs)))
}

fn prim_path_parent(args: &[Value]) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/parent", |s| match crate::path::parent(s) {
        Some(p) if !p.is_empty() => (SIG_OK, Value::string(p.to_string())),
        Some(_) => (SIG_OK, Value::NIL), // empty parent (e.g., parent("foo") is "")
        None => (SIG_OK, Value::NIL),
    })
}

fn prim_path_filename(args: &[Value]) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/filename", |s| {
        match crate::path::filename(s) {
            Some(f) => (SIG_OK, Value::string(f.to_string())),
            None => (SIG_OK, Value::NIL),
        }
    })
}

fn prim_path_stem(args: &[Value]) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/stem", |s| match crate::path::stem(s) {
        Some(st) => (SIG_OK, Value::string(st.to_string())),
        None => (SIG_OK, Value::NIL),
    })
}

fn prim_path_extension(args: &[Value]) -> (SignalBits, Value) {
    with_str_arg(
        &args[0],
        "path/extension",
        |s| match crate::path::extension(s) {
            Some(e) => (SIG_OK, Value::string(e.to_string())),
            None => (SIG_OK, Value::NIL),
        },
    )
}

fn prim_path_with_extension(args: &[Value]) -> (SignalBits, Value) {
    let path_str = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "path/with-extension: expected string, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    let ext_str = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "path/with-extension: expected string, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };
    (
        SIG_OK,
        Value::string(crate::path::with_extension(&path_str, &ext_str)),
    )
}

fn prim_path_normalize(args: &[Value]) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/normalize", |s| {
        (SIG_OK, Value::string(crate::path::normalize(s)))
    })
}

fn prim_path_absolute(args: &[Value]) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/absolute", |s| {
        match crate::path::absolute(s) {
            Ok(abs) => (SIG_OK, Value::string(abs)),
            Err(e) => (
                SIG_ERROR,
                error_val("error", format!("path/absolute: {}", e)),
            ),
        }
    })
}

fn prim_path_canonicalize(args: &[Value]) -> (SignalBits, Value) {
    with_str_arg(
        &args[0],
        "path/canonicalize",
        |s| match crate::path::canonicalize(s) {
            Ok(c) => (SIG_OK, Value::string(c)),
            Err(e) => (
                SIG_ERROR,
                error_val("error", format!("path/canonicalize: {}", e)),
            ),
        },
    )
}

fn prim_path_relative(args: &[Value]) -> (SignalBits, Value) {
    let path_str = match args[0].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "path/relative: expected string, got {}",
                        args[0].type_name()
                    ),
                ),
            )
        }
    };
    let base_str = match args[1].with_string(|s| s.to_string()) {
        Some(s) => s,
        None => {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!(
                        "path/relative: expected string, got {}",
                        args[1].type_name()
                    ),
                ),
            )
        }
    };
    match crate::path::relative(&path_str, &base_str) {
        Some(rel) => (SIG_OK, Value::string(rel)),
        None => (SIG_OK, Value::NIL),
    }
}

fn prim_path_components(args: &[Value]) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/components", |s| {
        let parts = crate::path::components(s);
        let values: Vec<Value> = parts.into_iter().map(Value::string).collect();
        (SIG_OK, crate::value::list(values))
    })
}

fn prim_path_is_absolute(args: &[Value]) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/absolute?", |s| {
        (SIG_OK, Value::bool(crate::path::is_absolute(s)))
    })
}

fn prim_path_is_relative(args: &[Value]) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/relative?", |s| {
        (SIG_OK, Value::bool(crate::path::is_relative(s)))
    })
}

fn prim_path_cwd(_args: &[Value]) -> (SignalBits, Value) {
    match crate::path::cwd() {
        Ok(c) => (SIG_OK, Value::string(c)),
        Err(e) => (SIG_ERROR, error_val("error", format!("path/cwd: {}", e))),
    }
}

fn prim_path_exists(args: &[Value]) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/exists?", |s| {
        (SIG_OK, Value::bool(crate::path::exists(s)))
    })
}

fn prim_path_is_file(args: &[Value]) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/file?", |s| {
        (SIG_OK, Value::bool(crate::path::is_file(s)))
    })
}

fn prim_path_is_dir(args: &[Value]) -> (SignalBits, Value) {
    with_str_arg(&args[0], "path/dir?", |s| {
        (SIG_OK, Value::bool(crate::path::is_dir(s)))
    })
}

pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "path/join",
        func: prim_path_join,
        effect: Effect::none(),
        arity: Arity::AtLeast(1),
        doc: "Join path components",
        params: &["components"],
        category: "path",
        example: "(path/join \"a\" \"b\" \"c\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "path/parent",
        func: prim_path_parent,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Get parent directory (nil if none)",
        params: &["path"],
        category: "path",
        example: "(path/parent \"/home/user/data.txt\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "path/filename",
        func: prim_path_filename,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Get file name (last component, nil if none)",
        params: &["path"],
        category: "path",
        example: "(path/filename \"/home/user/data.txt\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "path/stem",
        func: prim_path_stem,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Get file stem (filename without extension, nil if none)",
        params: &["path"],
        category: "path",
        example: "(path/stem \"archive.tar.gz\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "path/extension",
        func: prim_path_extension,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Get file extension without dot (nil if none)",
        params: &["path"],
        category: "path",
        example: "(path/extension \"data.txt\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "path/with-extension",
        func: prim_path_with_extension,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Replace file extension (empty string removes it)",
        params: &["path", "ext"],
        category: "path",
        example: "(path/with-extension \"foo.txt\" \"rs\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "path/normalize",
        func: prim_path_normalize,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Lexical path normalization (resolve . and ..)",
        params: &["path"],
        category: "path",
        example: "(path/normalize \"./a/../b\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "path/absolute",
        func: prim_path_absolute,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Compute absolute path (does not require path to exist)",
        params: &["path"],
        category: "path",
        example: "(path/absolute \"src\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "path/canonicalize",
        func: prim_path_canonicalize,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Resolve path through filesystem (symlinks resolved, must exist)",
        params: &["path"],
        category: "path",
        example: "(path/canonicalize \".\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "path/relative",
        func: prim_path_relative,
        effect: Effect::none(),
        arity: Arity::Exact(2),
        doc: "Compute relative path from base to target (nil if impossible)",
        params: &["target", "base"],
        category: "path",
        example: "(path/relative \"/foo/bar/baz\" \"/foo/bar\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "path/components",
        func: prim_path_components,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Split path into list of components",
        params: &["path"],
        category: "path",
        example: "(path/components \"/a/b/c\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "path/absolute?",
        func: prim_path_is_absolute,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "True if path is absolute",
        params: &["path"],
        category: "path",
        example: "(path/absolute? \"/foo\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "path/relative?",
        func: prim_path_is_relative,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "True if path is relative",
        params: &["path"],
        category: "path",
        example: "(path/relative? \"foo\")",
        aliases: &[],
    },
    PrimitiveDef {
        name: "path/cwd",
        func: prim_path_cwd,
        effect: Effect::raises(),
        arity: Arity::Exact(0),
        doc: "Get current working directory",
        params: &[],
        category: "path",
        example: "(path/cwd)",
        aliases: &[],
    },
    PrimitiveDef {
        name: "path/exists?",
        func: prim_path_exists,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if path exists",
        params: &["path"],
        category: "path",
        example: "(path/exists? \"data.txt\")",
        aliases: &["file-exists?", "file/exists?"],
    },
    PrimitiveDef {
        name: "path/file?",
        func: prim_path_is_file,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if path is a regular file",
        params: &["path"],
        category: "path",
        example: "(path/file? \"data.txt\")",
        aliases: &["file?", "file/file?"],
    },
    PrimitiveDef {
        name: "path/dir?",
        func: prim_path_is_dir,
        effect: Effect::none(),
        arity: Arity::Exact(1),
        doc: "Check if path is a directory",
        params: &["path"],
        category: "path",
        example: "(path/dir? \"/home\")",
        aliases: &["directory?", "file/directory?"],
    },
];
