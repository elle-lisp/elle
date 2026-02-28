//! Path manipulation primitives
use crate::effects::Effect;
use crate::primitives::def::PrimitiveDef;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::{error_val, Value};

/// Get absolute path
pub fn prim_absolute_path(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("absolute-path: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(result) = args[0].with_string(|path| match std::fs::canonicalize(path) {
        Ok(abs_path) => (
            SIG_OK,
            Value::string(abs_path.to_string_lossy().into_owned()),
        ),
        Err(e) => (
            SIG_ERROR,
            error_val(
                "error",
                format!("absolute-path: failed to resolve '{}': {}", path, e),
            ),
        ),
    }) {
        result
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "absolute-path: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Get current working directory
pub fn prim_current_directory(_args: &[Value]) -> (SignalBits, Value) {
    match std::env::current_dir() {
        Ok(path) => (SIG_OK, Value::string(path.to_string_lossy().into_owned())),
        Err(e) => (
            SIG_ERROR,
            error_val(
                "error",
                format!("current-directory: failed to get current directory: {}", e),
            ),
        ),
    }
}

/// Change current working directory
pub fn prim_change_directory(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("change-directory: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(result) = args[0].with_string(|path| match std::env::set_current_dir(path) {
        Ok(_) => (SIG_OK, Value::TRUE),
        Err(e) => (
            SIG_ERROR,
            error_val(
                "error",
                format!("change-directory: failed to change to '{}': {}", path, e),
            ),
        ),
    }) {
        result
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "change-directory: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Join path components (return a properly formatted path)
pub fn prim_join_path(args: &[Value]) -> (SignalBits, Value) {
    if args.is_empty() {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                "join-path: expected at least 1 argument, got 0",
            ),
        );
    }

    let mut path = std::path::PathBuf::new();
    for arg in args {
        if let Some(s) = arg.with_string(|s| s.to_string()) {
            path.push(s);
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "type-error",
                    format!("join-path: expected string, got {}", arg.type_name()),
                ),
            );
        }
    }

    (SIG_OK, Value::string(path.to_string_lossy().into_owned()))
}

/// Get file extension
pub fn prim_file_extension(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("file-extension: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(result) = args[0].with_string(|path_str| {
        let path = std::path::Path::new(path_str);
        match path.extension() {
            Some(ext) => (SIG_OK, Value::string(ext.to_string_lossy().into_owned())),
            None => (SIG_OK, Value::NIL),
        }
    }) {
        result
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "file-extension: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Get file name (without directory)
pub fn prim_file_name(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("file-name: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(result) = args[0].with_string(|path_str| {
        let path = std::path::Path::new(path_str);
        match path.file_name() {
            Some(name) => (SIG_OK, Value::string(name.to_string_lossy().into_owned())),
            None => (SIG_OK, Value::NIL),
        }
    }) {
        result
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("file-name: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Get parent directory path
pub fn prim_parent_directory(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("parent-directory: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(result) = args[0].with_string(|path_str| {
        let path = std::path::Path::new(path_str);
        match path.parent() {
            Some(parent) => (SIG_OK, Value::string(parent.to_string_lossy().into_owned())),
            None => (SIG_OK, Value::NIL),
        }
    }) {
        result
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "parent-directory: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Declarative primitive definitions for path manipulation.
pub const PRIMITIVES: &[PrimitiveDef] = &[
    PrimitiveDef {
        name: "file/realpath",
        func: prim_absolute_path,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Get absolute path",
        params: &["path"],
        category: "file",
        example: "(file/realpath \"./data.txt\")",
        aliases: &["absolute-path"],
    },
    PrimitiveDef {
        name: "file/cwd",
        func: prim_current_directory,
        effect: Effect::raises(),
        arity: Arity::Exact(0),
        doc: "Get current working directory",
        params: &[],
        category: "file",
        example: "(file/cwd)",
        aliases: &["current-directory"],
    },
    PrimitiveDef {
        name: "file/cd",
        func: prim_change_directory,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Change current working directory",
        params: &["path"],
        category: "file",
        example: "(file/cd \"/home\")",
        aliases: &["change-directory"],
    },
    PrimitiveDef {
        name: "file/join",
        func: prim_join_path,
        effect: Effect::raises(),
        arity: Arity::AtLeast(1),
        doc: "Join path components",
        params: &["components"],
        category: "file",
        example: "(file/join \"a\" \"b\" \"c\")",
        aliases: &["join-path"],
    },
    PrimitiveDef {
        name: "file/ext",
        func: prim_file_extension,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Get file extension",
        params: &["path"],
        category: "file",
        example: "(file/ext \"data.txt\")",
        aliases: &["file-extension"],
    },
    PrimitiveDef {
        name: "file/name",
        func: prim_file_name,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Get file name (without directory)",
        params: &["path"],
        category: "file",
        example: "(file/name \"/home/user/data.txt\")",
        aliases: &["file-name"],
    },
    PrimitiveDef {
        name: "file/parent",
        func: prim_parent_directory,
        effect: Effect::raises(),
        arity: Arity::Exact(1),
        doc: "Get parent directory path",
        params: &["path"],
        category: "file",
        example: "(file/parent \"/home/user/data.txt\")",
        aliases: &["parent-directory"],
    },
];
