//! File I/O primitives
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::{error_val, Value};

/// Read entire file as a string
pub fn prim_slurp(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("slurp: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(path) = args[0].as_string() {
        match std::fs::read_to_string(path) {
            Ok(content) => (SIG_OK, Value::string(content)),
            Err(e) => (
                SIG_ERROR,
                error_val("error", format!("slurp: failed to read '{}': {}", path, e)),
            ),
        }
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("slurp: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Write string content to a file (overwrites if exists)
pub fn prim_spit(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("spit: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let path = if let Some(s) = args[0].as_string() {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("spit: expected string, got {}", args[0].type_name()),
            ),
        );
    };

    let content = if let Some(s) = args[1].as_string() {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("spit: expected string, got {}", args[1].type_name()),
            ),
        );
    };

    match std::fs::write(path, content) {
        Ok(_) => (SIG_OK, Value::TRUE),
        Err(e) => (
            SIG_ERROR,
            error_val("error", format!("spit: failed to write '{}': {}", path, e)),
        ),
    }
}

/// Append string content to a file
pub fn prim_append_file(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("append-file: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let path = if let Some(s) = args[0].as_string() {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("append-file: expected string, got {}", args[0].type_name()),
            ),
        );
    };

    let content = if let Some(s) = args[1].as_string() {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("append-file: expected string, got {}", args[1].type_name()),
            ),
        );
    };

    use std::fs::OpenOptions;
    use std::io::Write;

    let mut file = match OpenOptions::new().create(true).append(true).open(path) {
        Ok(f) => f,
        Err(e) => {
            return (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("append-file: failed to open '{}': {}", path, e),
                ),
            )
        }
    };

    match file.write_all(content.as_bytes()) {
        Ok(_) => (SIG_OK, Value::TRUE),
        Err(e) => (
            SIG_ERROR,
            error_val(
                "error",
                format!("append-file: failed to write '{}': {}", path, e),
            ),
        ),
    }
}

/// Check if a file exists
pub fn prim_file_exists(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("file-exists?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(path) = args[0].as_string() {
        (SIG_OK, Value::bool(std::path::Path::new(path).exists()))
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("file-exists?: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Check if path is a directory
pub fn prim_is_directory(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("directory?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(path) = args[0].as_string() {
        match std::fs::metadata(path) {
            Ok(metadata) => (SIG_OK, Value::bool(metadata.is_dir())),
            Err(_) => (SIG_OK, Value::FALSE),
        }
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("directory?: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Check if path is a file
pub fn prim_is_file(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("file?: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(path) = args[0].as_string() {
        match std::fs::metadata(path) {
            Ok(metadata) => (SIG_OK, Value::bool(metadata.is_file())),
            Err(_) => (SIG_OK, Value::FALSE),
        }
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("file?: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Delete a file
pub fn prim_delete_file(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("delete-file: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(path) = args[0].as_string() {
        match std::fs::remove_file(path) {
            Ok(_) => (SIG_OK, Value::TRUE),
            Err(e) => (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("delete-file: failed to delete '{}': {}", path, e),
                ),
            ),
        }
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("delete-file: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

/// Delete a directory (must be empty)
pub fn prim_delete_directory(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("delete-directory: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(path) = args[0].as_string() {
        match std::fs::remove_dir(path) {
            Ok(_) => (SIG_OK, Value::TRUE),
            Err(e) => (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("delete-directory: failed to delete '{}': {}", path, e),
                ),
            ),
        }
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "delete-directory: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Create a directory
pub fn prim_create_directory(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("create-directory: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(path) = args[0].as_string() {
        match std::fs::create_dir(path) {
            Ok(_) => (SIG_OK, Value::TRUE),
            Err(e) => (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("create-directory: failed to create '{}': {}", path, e),
                ),
            ),
        }
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "create-directory: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Create a directory and all parent directories
pub fn prim_create_directory_all(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!(
                    "create-directory-all: expected 1 argument, got {}",
                    args.len()
                ),
            ),
        );
    }
    if let Some(path) = args[0].as_string() {
        match std::fs::create_dir_all(path) {
            Ok(_) => (SIG_OK, Value::TRUE),
            Err(e) => (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("create-directory-all: failed to create '{}': {}", path, e),
                ),
            ),
        }
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "create-directory-all: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

/// Rename a file
pub fn prim_rename_file(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("rename-file: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let old_path = if let Some(s) = args[0].as_string() {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("rename-file: expected string, got {}", args[0].type_name()),
            ),
        );
    };

    let new_path = if let Some(s) = args[1].as_string() {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("rename-file: expected string, got {}", args[1].type_name()),
            ),
        );
    };

    match std::fs::rename(old_path, new_path) {
        Ok(_) => (SIG_OK, Value::TRUE),
        Err(e) => (
            SIG_ERROR,
            error_val(
                "error",
                format!("rename-file: failed to rename '{}': {}", old_path, e),
            ),
        ),
    }
}

/// Copy a file
pub fn prim_copy_file(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 2 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("copy-file: expected 2 arguments, got {}", args.len()),
            ),
        );
    }

    let src = if let Some(s) = args[0].as_string() {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("copy-file: expected string, got {}", args[0].type_name()),
            ),
        );
    };

    let dst = if let Some(s) = args[1].as_string() {
        s
    } else {
        return (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("copy-file: expected string, got {}", args[1].type_name()),
            ),
        );
    };

    match std::fs::copy(src, dst) {
        Ok(_) => (SIG_OK, Value::TRUE),
        Err(e) => (
            SIG_ERROR,
            error_val(
                "error",
                format!("copy-file: failed to copy '{}': {}", src, e),
            ),
        ),
    }
}

/// Get file size in bytes
pub fn prim_file_size(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("file-size: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(path) = args[0].as_string() {
        match std::fs::metadata(path) {
            Ok(metadata) => (SIG_OK, Value::int(metadata.len() as i64)),
            Err(e) => (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("file-size: failed to get size of '{}': {}", path, e),
                ),
            ),
        }
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("file-size: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}

/// List directory contents
pub fn prim_list_directory(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("list-directory: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(path) = args[0].as_string() {
        match std::fs::read_dir(path) {
            Ok(entries) => {
                let mut items = Vec::new();
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            if let Ok(name) = entry.file_name().into_string() {
                                items.push(Value::string(name));
                            }
                        }
                        Err(e) => {
                            return (
                                SIG_ERROR,
                                error_val(
                                    "error",
                                    format!("list-directory: error reading '{}': {}", path, e),
                                ),
                            );
                        }
                    }
                }
                (SIG_OK, crate::value::list(items))
            }
            Err(e) => (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("list-directory: failed to read '{}': {}", path, e),
                ),
            ),
        }
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "list-directory: expected string, got {}",
                    args[0].type_name()
                ),
            ),
        )
    }
}

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
    if let Some(path) = args[0].as_string() {
        match std::fs::canonicalize(path) {
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
        }
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
    if let Some(path) = args[0].as_string() {
        match std::env::set_current_dir(path) {
            Ok(_) => (SIG_OK, Value::TRUE),
            Err(e) => (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("change-directory: failed to change to '{}': {}", path, e),
                ),
            ),
        }
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
        if let Some(s) = arg.as_string() {
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
    if let Some(path_str) = args[0].as_string() {
        let path = std::path::Path::new(path_str);
        match path.extension() {
            Some(ext) => (SIG_OK, Value::string(ext.to_string_lossy().into_owned())),
            None => (SIG_OK, Value::NIL),
        }
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
    if let Some(path_str) = args[0].as_string() {
        let path = std::path::Path::new(path_str);
        match path.file_name() {
            Some(name) => (SIG_OK, Value::string(name.to_string_lossy().into_owned())),
            None => (SIG_OK, Value::NIL),
        }
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
    if let Some(path_str) = args[0].as_string() {
        let path = std::path::Path::new(path_str);
        match path.parent() {
            Some(parent) => (SIG_OK, Value::string(parent.to_string_lossy().into_owned())),
            None => (SIG_OK, Value::NIL),
        }
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

/// Read lines from a file and return as a list of strings
pub fn prim_read_lines(args: &[Value]) -> (SignalBits, Value) {
    if args.len() != 1 {
        return (
            SIG_ERROR,
            error_val(
                "arity-error",
                format!("read-lines: expected 1 argument, got {}", args.len()),
            ),
        );
    }
    if let Some(path) = args[0].as_string() {
        match std::fs::read_to_string(path) {
            Ok(content) => {
                let lines: Vec<Value> = content
                    .lines()
                    .map(|line| Value::string(line.to_string()))
                    .collect();
                (SIG_OK, crate::value::list(lines))
            }
            Err(e) => (
                SIG_ERROR,
                error_val(
                    "error",
                    format!("read-lines: failed to read '{}': {}", path, e),
                ),
            ),
        }
    } else {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("read-lines: expected string, got {}", args[0].type_name()),
            ),
        )
    }
}
