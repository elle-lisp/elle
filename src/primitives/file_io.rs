//! File I/O primitives
use crate::value::{Condition, Value};

/// Read entire file as a string
pub fn prim_slurp(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "slurp: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(path) = args[0].as_string() {
        std::fs::read_to_string(path)
            .map(Value::string)
            .map_err(|e| Condition::error(format!("slurp: failed to read '{}': {}", path, e)))
    } else {
        Err(Condition::type_error(format!(
            "slurp: expected string, got {}",
            args[0].type_name()
        )))
    }
}

/// Write string content to a file (overwrites if exists)
pub fn prim_spit(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "spit: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let path = if let Some(s) = args[0].as_string() {
        s
    } else {
        return Err(Condition::type_error(format!(
            "spit: expected string, got {}",
            args[0].type_name()
        )));
    };

    let content = if let Some(s) = args[1].as_string() {
        s
    } else {
        return Err(Condition::type_error(format!(
            "spit: expected string, got {}",
            args[1].type_name()
        )));
    };

    std::fs::write(path, content)
        .map(|_| Value::TRUE)
        .map_err(|e| Condition::error(format!("spit: failed to write '{}': {}", path, e)))
}

/// Append string content to a file
pub fn prim_append_file(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "append-file: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let path = if let Some(s) = args[0].as_string() {
        s
    } else {
        return Err(Condition::type_error(format!(
            "append-file: expected string, got {}",
            args[0].type_name()
        )));
    };

    let content = if let Some(s) = args[1].as_string() {
        s
    } else {
        return Err(Condition::type_error(format!(
            "append-file: expected string, got {}",
            args[1].type_name()
        )));
    };

    use std::fs::OpenOptions;
    use std::io::Write;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| Condition::error(format!("append-file: failed to open '{}': {}", path, e)))?;

    file.write_all(content.as_bytes())
        .map(|_| Value::TRUE)
        .map_err(|e| Condition::error(format!("append-file: failed to write '{}': {}", path, e)))
}

/// Check if a file exists
pub fn prim_file_exists(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "file-exists?: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(path) = args[0].as_string() {
        Ok(Value::bool(std::path::Path::new(path).exists()))
    } else {
        Err(Condition::type_error(format!(
            "file-exists?: expected string, got {}",
            args[0].type_name()
        )))
    }
}

/// Check if path is a directory
pub fn prim_is_directory(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "directory?: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(path) = args[0].as_string() {
        match std::fs::metadata(path) {
            Ok(metadata) => Ok(Value::bool(metadata.is_dir())),
            Err(_) => Ok(Value::FALSE),
        }
    } else {
        Err(Condition::type_error(format!(
            "directory?: expected string, got {}",
            args[0].type_name()
        )))
    }
}

/// Check if path is a file
pub fn prim_is_file(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "file?: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(path) = args[0].as_string() {
        match std::fs::metadata(path) {
            Ok(metadata) => Ok(Value::bool(metadata.is_file())),
            Err(_) => Ok(Value::FALSE),
        }
    } else {
        Err(Condition::type_error(format!(
            "file?: expected string, got {}",
            args[0].type_name()
        )))
    }
}

/// Delete a file
pub fn prim_delete_file(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "delete-file: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(path) = args[0].as_string() {
        std::fs::remove_file(path)
            .map(|_| Value::TRUE)
            .map_err(|e| {
                Condition::error(format!("delete-file: failed to delete '{}': {}", path, e))
            })
    } else {
        Err(Condition::type_error(format!(
            "delete-file: expected string, got {}",
            args[0].type_name()
        )))
    }
}

/// Delete a directory (must be empty)
pub fn prim_delete_directory(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "delete-directory: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(path) = args[0].as_string() {
        std::fs::remove_dir(path).map(|_| Value::TRUE).map_err(|e| {
            Condition::error(format!(
                "delete-directory: failed to delete '{}': {}",
                path, e
            ))
        })
    } else {
        Err(Condition::type_error(format!(
            "delete-directory: expected string, got {}",
            args[0].type_name()
        )))
    }
}

/// Create a directory
pub fn prim_create_directory(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "create-directory: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(path) = args[0].as_string() {
        std::fs::create_dir(path).map(|_| Value::TRUE).map_err(|e| {
            Condition::error(format!(
                "create-directory: failed to create '{}': {}",
                path, e
            ))
        })
    } else {
        Err(Condition::type_error(format!(
            "create-directory: expected string, got {}",
            args[0].type_name()
        )))
    }
}

/// Create a directory and all parent directories
pub fn prim_create_directory_all(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "create-directory-all: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(path) = args[0].as_string() {
        std::fs::create_dir_all(path)
            .map(|_| Value::TRUE)
            .map_err(|e| {
                Condition::error(format!(
                    "create-directory-all: failed to create '{}': {}",
                    path, e
                ))
            })
    } else {
        Err(Condition::type_error(format!(
            "create-directory-all: expected string, got {}",
            args[0].type_name()
        )))
    }
}

/// Rename a file
pub fn prim_rename_file(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "rename-file: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let old_path = if let Some(s) = args[0].as_string() {
        s
    } else {
        return Err(Condition::type_error(format!(
            "rename-file: expected string, got {}",
            args[0].type_name()
        )));
    };

    let new_path = if let Some(s) = args[1].as_string() {
        s
    } else {
        return Err(Condition::type_error(format!(
            "rename-file: expected string, got {}",
            args[1].type_name()
        )));
    };

    std::fs::rename(old_path, new_path)
        .map(|_| Value::TRUE)
        .map_err(|e| {
            Condition::error(format!(
                "rename-file: failed to rename '{}': {}",
                old_path, e
            ))
        })
}

/// Copy a file
pub fn prim_copy_file(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 2 {
        return Err(Condition::arity_error(format!(
            "copy-file: expected 2 arguments, got {}",
            args.len()
        )));
    }

    let src = if let Some(s) = args[0].as_string() {
        s
    } else {
        return Err(Condition::type_error(format!(
            "copy-file: expected string, got {}",
            args[0].type_name()
        )));
    };

    let dst = if let Some(s) = args[1].as_string() {
        s
    } else {
        return Err(Condition::type_error(format!(
            "copy-file: expected string, got {}",
            args[1].type_name()
        )));
    };

    std::fs::copy(src, dst)
        .map(|_| Value::TRUE)
        .map_err(|e| Condition::error(format!("copy-file: failed to copy '{}': {}", src, e)))
}

/// Get file size in bytes
pub fn prim_file_size(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "file-size: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(path) = args[0].as_string() {
        std::fs::metadata(path)
            .map(|metadata| Value::int(metadata.len() as i64))
            .map_err(|e| {
                Condition::error(format!(
                    "file-size: failed to get size of '{}': {}",
                    path, e
                ))
            })
    } else {
        Err(Condition::type_error(format!(
            "file-size: expected string, got {}",
            args[0].type_name()
        )))
    }
}

/// List directory contents
pub fn prim_list_directory(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "list-directory: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(path) = args[0].as_string() {
        std::fs::read_dir(path)
            .map_err(|e| {
                Condition::error(format!("list-directory: failed to read '{}': {}", path, e))
            })
            .and_then(|entries| {
                let mut items = Vec::new();
                for entry in entries {
                    match entry {
                        Ok(entry) => {
                            if let Ok(name) = entry.file_name().into_string() {
                                items.push(Value::string(name));
                            }
                        }
                        Err(e) => {
                            return Err(Condition::error(format!(
                                "list-directory: error reading '{}': {}",
                                path, e
                            )))
                        }
                    }
                }
                Ok(crate::value::list(items))
            })
    } else {
        Err(Condition::type_error(format!(
            "list-directory: expected string, got {}",
            args[0].type_name()
        )))
    }
}

/// Get absolute path
pub fn prim_absolute_path(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "absolute-path: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(path) = args[0].as_string() {
        std::fs::canonicalize(path)
            .map(|abs_path| Value::string(abs_path.to_string_lossy().into_owned()))
            .map_err(|e| {
                Condition::error(format!(
                    "absolute-path: failed to resolve '{}': {}",
                    path, e
                ))
            })
    } else {
        Err(Condition::type_error(format!(
            "absolute-path: expected string, got {}",
            args[0].type_name()
        )))
    }
}

/// Get current working directory
pub fn prim_current_directory(_args: &[Value]) -> Result<Value, Condition> {
    std::env::current_dir()
        .map(|path| Value::string(path.to_string_lossy().into_owned()))
        .map_err(|e| {
            Condition::error(format!(
                "current-directory: failed to get current directory: {}",
                e
            ))
        })
}

/// Change current working directory
pub fn prim_change_directory(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "change-directory: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(path) = args[0].as_string() {
        std::env::set_current_dir(path)
            .map(|_| Value::TRUE)
            .map_err(|e| {
                Condition::error(format!(
                    "change-directory: failed to change to '{}': {}",
                    path, e
                ))
            })
    } else {
        Err(Condition::type_error(format!(
            "change-directory: expected string, got {}",
            args[0].type_name()
        )))
    }
}

/// Join path components (return a properly formatted path)
pub fn prim_join_path(args: &[Value]) -> Result<Value, Condition> {
    if args.is_empty() {
        return Err(Condition::arity_error(
            "join-path: expected at least 1 argument, got 0".to_string(),
        ));
    }

    let mut path = std::path::PathBuf::new();
    for arg in args {
        if let Some(s) = arg.as_string() {
            path.push(s);
        } else {
            return Err(Condition::type_error(format!(
                "join-path: expected string, got {}",
                arg.type_name()
            )));
        }
    }

    Ok(Value::string(path.to_string_lossy().into_owned()))
}

/// Get file extension
pub fn prim_file_extension(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "file-extension: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(path_str) = args[0].as_string() {
        let path = std::path::Path::new(path_str);
        match path.extension() {
            Some(ext) => Ok(Value::string(ext.to_string_lossy().into_owned())),
            None => Ok(Value::NIL),
        }
    } else {
        Err(Condition::type_error(format!(
            "file-extension: expected string, got {}",
            args[0].type_name()
        )))
    }
}

/// Get file name (without directory)
pub fn prim_file_name(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "file-name: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(path_str) = args[0].as_string() {
        let path = std::path::Path::new(path_str);
        match path.file_name() {
            Some(name) => Ok(Value::string(name.to_string_lossy().into_owned())),
            None => Ok(Value::NIL),
        }
    } else {
        Err(Condition::type_error(format!(
            "file-name: expected string, got {}",
            args[0].type_name()
        )))
    }
}

/// Get parent directory path
pub fn prim_parent_directory(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "parent-directory: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(path_str) = args[0].as_string() {
        let path = std::path::Path::new(path_str);
        match path.parent() {
            Some(parent) => Ok(Value::string(parent.to_string_lossy().into_owned())),
            None => Ok(Value::NIL),
        }
    } else {
        Err(Condition::type_error(format!(
            "parent-directory: expected string, got {}",
            args[0].type_name()
        )))
    }
}

/// Read lines from a file and return as a list of strings
pub fn prim_read_lines(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(format!(
            "read-lines: expected 1 argument, got {}",
            args.len()
        )));
    }
    if let Some(path) = args[0].as_string() {
        std::fs::read_to_string(path)
            .map_err(|e| Condition::error(format!("read-lines: failed to read '{}': {}", path, e)))
            .map(|content| {
                let lines: Vec<Value> = content
                    .lines()
                    .map(|line| Value::string(line.to_string()))
                    .collect();
                crate::value::list(lines)
            })
    } else {
        Err(Condition::type_error(format!(
            "read-lines: expected string, got {}",
            args[0].type_name()
        )))
    }
}
