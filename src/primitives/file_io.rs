//! File I/O primitives
use crate::value::Value;
use std::rc::Rc;

/// Read entire file as a string
pub fn prim_read_file(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("read-file requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(path) => {
            let path_str = path.as_ref();
            std::fs::read_to_string(path_str)
                .map(|content| Value::String(Rc::from(content)))
                .map_err(|e| format!("Failed to read file '{}': {}", path_str, e))
        }
        _ => Err("read-file requires a string path".to_string()),
    }
}

/// Write string content to a file (overwrites if exists)
pub fn prim_write_file(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("write-file requires exactly 2 arguments (path, content)".to_string());
    }

    let path = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("write-file: first argument must be a string path".to_string()),
    };

    let content = match &args[1] {
        Value::String(s) => s.as_ref(),
        _ => return Err("write-file: second argument must be a string".to_string()),
    };

    std::fs::write(path, content)
        .map(|_| Value::Bool(true))
        .map_err(|e| format!("Failed to write file '{}': {}", path, e))
}

/// Append string content to a file
pub fn prim_append_file(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("append-file requires exactly 2 arguments (path, content)".to_string());
    }

    let path = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("append-file: first argument must be a string path".to_string()),
    };

    let content = match &args[1] {
        Value::String(s) => s.as_ref(),
        _ => return Err("append-file: second argument must be a string".to_string()),
    };

    use std::fs::OpenOptions;
    use std::io::Write;

    let mut file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("Failed to open file '{}' for appending: {}", path, e))?;

    file.write_all(content.as_bytes())
        .map(|_| Value::Bool(true))
        .map_err(|e| format!("Failed to write to file '{}': {}", path, e))
}

/// Check if a file exists
pub fn prim_file_exists(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("file-exists? requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(path) => {
            let path_str = path.as_ref();
            Ok(Value::Bool(std::path::Path::new(path_str).exists()))
        }
        _ => Err("file-exists? requires a string path".to_string()),
    }
}

/// Check if path is a directory
pub fn prim_is_directory(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("directory? requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(path) => {
            let path_str = path.as_ref();
            match std::fs::metadata(path_str) {
                Ok(metadata) => Ok(Value::Bool(metadata.is_dir())),
                Err(_) => Ok(Value::Bool(false)),
            }
        }
        _ => Err("directory? requires a string path".to_string()),
    }
}

/// Check if path is a file
pub fn prim_is_file(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("file? requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(path) => {
            let path_str = path.as_ref();
            match std::fs::metadata(path_str) {
                Ok(metadata) => Ok(Value::Bool(metadata.is_file())),
                Err(_) => Ok(Value::Bool(false)),
            }
        }
        _ => Err("file? requires a string path".to_string()),
    }
}

/// Delete a file
pub fn prim_delete_file(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("delete-file requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(path) => {
            let path_str = path.as_ref();
            std::fs::remove_file(path_str)
                .map(|_| Value::Bool(true))
                .map_err(|e| format!("Failed to delete file '{}': {}", path_str, e))
        }
        _ => Err("delete-file requires a string path".to_string()),
    }
}

/// Delete a directory (must be empty)
pub fn prim_delete_directory(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("delete-directory requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(path) => {
            let path_str = path.as_ref();
            std::fs::remove_dir(path_str)
                .map(|_| Value::Bool(true))
                .map_err(|e| format!("Failed to delete directory '{}': {}", path_str, e))
        }
        _ => Err("delete-directory requires a string path".to_string()),
    }
}

/// Create a directory
pub fn prim_create_directory(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("create-directory requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(path) => {
            let path_str = path.as_ref();
            std::fs::create_dir(path_str)
                .map(|_| Value::Bool(true))
                .map_err(|e| format!("Failed to create directory '{}': {}", path_str, e))
        }
        _ => Err("create-directory requires a string path".to_string()),
    }
}

/// Create a directory and all parent directories
pub fn prim_create_directory_all(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("create-directory-all requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(path) => {
            let path_str = path.as_ref();
            std::fs::create_dir_all(path_str)
                .map(|_| Value::Bool(true))
                .map_err(|e| format!("Failed to create directory structure '{}': {}", path_str, e))
        }
        _ => Err("create-directory-all requires a string path".to_string()),
    }
}

/// Rename a file
pub fn prim_rename_file(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("rename-file requires exactly 2 arguments (old-path, new-path)".to_string());
    }

    let old_path = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("rename-file: first argument must be a string path".to_string()),
    };

    let new_path = match &args[1] {
        Value::String(s) => s.as_ref(),
        _ => return Err("rename-file: second argument must be a string path".to_string()),
    };

    std::fs::rename(old_path, new_path)
        .map(|_| Value::Bool(true))
        .map_err(|e| {
            format!(
                "Failed to rename file from '{}' to '{}': {}",
                old_path, new_path, e
            )
        })
}

/// Copy a file
pub fn prim_copy_file(args: &[Value]) -> Result<Value, String> {
    if args.len() != 2 {
        return Err("copy-file requires exactly 2 arguments (source, dest)".to_string());
    }

    let src = match &args[0] {
        Value::String(s) => s.as_ref(),
        _ => return Err("copy-file: first argument must be a string path".to_string()),
    };

    let dst = match &args[1] {
        Value::String(s) => s.as_ref(),
        _ => return Err("copy-file: second argument must be a string path".to_string()),
    };

    std::fs::copy(src, dst)
        .map(|_| Value::Bool(true))
        .map_err(|e| format!("Failed to copy file from '{}' to '{}': {}", src, dst, e))
}

/// Get file size in bytes
pub fn prim_file_size(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("file-size requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(path) => {
            let path_str = path.as_ref();
            std::fs::metadata(path_str)
                .map(|metadata| Value::Int(metadata.len() as i64))
                .map_err(|e| format!("Failed to get file size for '{}': {}", path_str, e))
        }
        _ => Err("file-size requires a string path".to_string()),
    }
}

/// List directory contents
pub fn prim_list_directory(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("list-directory requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(path) => {
            let path_str = path.as_ref();
            std::fs::read_dir(path_str)
                .map_err(|e| format!("Failed to read directory '{}': {}", path_str, e))
                .and_then(|entries| {
                    let mut items = Vec::new();
                    for entry in entries {
                        match entry {
                            Ok(entry) => {
                                if let Ok(name) = entry.file_name().into_string() {
                                    items.push(Value::String(Rc::from(name)));
                                }
                            }
                            Err(e) => return Err(format!("Error reading directory entry: {}", e)),
                        }
                    }
                    Ok(crate::value::list(items))
                })
        }
        _ => Err("list-directory requires a string path".to_string()),
    }
}

/// Get absolute path
pub fn prim_absolute_path(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("absolute-path requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(path) => {
            let path_str = path.as_ref();
            std::fs::canonicalize(path_str)
                .map(|abs_path| Value::String(Rc::from(abs_path.to_string_lossy().into_owned())))
                .map_err(|e| format!("Failed to get absolute path for '{}': {}", path_str, e))
        }
        _ => Err("absolute-path requires a string path".to_string()),
    }
}

/// Get current working directory
pub fn prim_current_directory(_args: &[Value]) -> Result<Value, String> {
    std::env::current_dir()
        .map(|path| Value::String(Rc::from(path.to_string_lossy().into_owned())))
        .map_err(|e| format!("Failed to get current directory: {}", e))
}

/// Change current working directory
pub fn prim_change_directory(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("change-directory requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(path) => {
            let path_str = path.as_ref();
            std::env::set_current_dir(path_str)
                .map(|_| Value::Bool(true))
                .map_err(|e| format!("Failed to change directory to '{}': {}", path_str, e))
        }
        _ => Err("change-directory requires a string path".to_string()),
    }
}

/// Join path components (return a properly formatted path)
pub fn prim_join_path(args: &[Value]) -> Result<Value, String> {
    if args.is_empty() {
        return Err("join-path requires at least 1 argument".to_string());
    }

    let mut path = std::path::PathBuf::new();
    for arg in args {
        match arg {
            Value::String(s) => path.push(s.as_ref()),
            _ => return Err("join-path requires all arguments to be strings".to_string()),
        }
    }

    Ok(Value::String(Rc::from(path.to_string_lossy().into_owned())))
}

/// Get file extension
pub fn prim_file_extension(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("file-extension requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(path) => {
            let path = std::path::Path::new(path.as_ref());
            match path.extension() {
                Some(ext) => Ok(Value::String(Rc::from(ext.to_string_lossy().into_owned()))),
                None => Ok(Value::Nil),
            }
        }
        _ => Err("file-extension requires a string path".to_string()),
    }
}

/// Get file name (without directory)
pub fn prim_file_name(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("file-name requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(path) => {
            let path = std::path::Path::new(path.as_ref());
            match path.file_name() {
                Some(name) => Ok(Value::String(Rc::from(name.to_string_lossy().into_owned()))),
                None => Ok(Value::Nil),
            }
        }
        _ => Err("file-name requires a string path".to_string()),
    }
}

/// Get parent directory path
pub fn prim_parent_directory(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("parent-directory requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(path) => {
            let path = std::path::Path::new(path.as_ref());
            match path.parent() {
                Some(parent) => Ok(Value::String(Rc::from(
                    parent.to_string_lossy().into_owned(),
                ))),
                None => Ok(Value::Nil),
            }
        }
        _ => Err("parent-directory requires a string path".to_string()),
    }
}

/// Read lines from a file and return as a list of strings
pub fn prim_read_lines(args: &[Value]) -> Result<Value, String> {
    if args.len() != 1 {
        return Err("read-lines requires exactly 1 argument".to_string());
    }
    match &args[0] {
        Value::String(path) => {
            let path_str = path.as_ref();
            std::fs::read_to_string(path_str)
                .map_err(|e| format!("Failed to read file '{}': {}", path_str, e))
                .map(|content| {
                    let lines: Vec<Value> = content
                        .lines()
                        .map(|line| Value::String(Rc::from(line.to_string())))
                        .collect();
                    crate::value::list(lines)
                })
        }
        _ => Err("read-lines requires a string path".to_string()),
    }
}
