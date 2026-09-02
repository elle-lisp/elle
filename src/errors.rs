//! Runtime/compile error formatting (human and JSON).

/// Format a runtime error, naming any `SymbolId(N)` it carries through
/// `symbols` — the instance that raised the error, and so the only memo that
/// can hold the name (docs/impl/symbol.md).
pub(super) fn format_runtime_error(error: &str, symbols: &elle::symbol::SymbolTable) -> String {
    // Check for SymbolId pattern and resolve it
    if let Some(start) = error.find("SymbolId(") {
        if let Some(end) = error[start..].find(')') {
            let id_str = &error[start + 9..start + end];
            if let Ok(id) = id_str.parse::<u64>() {
                let name = symbols
                    .name(elle::value::SymbolId(id))
                    .unwrap_or("<unknown>");
                let before = &error[..start];
                let after = &error[start + end + 1..];
                return format!("{}'{}'{}", before, name, after);
            }
        }
    }
    error.to_string()
}

/// Parse a compilation error string into an LError for structured display.
/// When the error has "file:line:col: message" format, extracts location.
/// Uses Generic kind so `description()` returns just the message without
/// an extra "Compile error:" prefix (the caller provides context).
pub(super) fn parse_compilation_error(error: &str) -> elle::error::LError {
    if let Some((file, line, col, message)) = elle::error::parse_located_error(error) {
        elle::error::LError::new(elle::error::ErrorKind::CompileError {
            message: message.to_string(),
        })
        .with_location(elle::error::SourceLoc::new(file, line, col))
    } else {
        elle::error::LError::compile_error(error)
    }
}

/// Format a compilation error as JSON for --json mode
pub(super) fn format_error_json(error: &elle::error::LError) -> String {
    let (file, line, col) = match &error.location {
        Some(loc) => (loc.file.as_str(), loc.line, loc.col),
        None => (elle::reader::UNKNOWN_FILE, 0, 0),
    };
    let (kind, message) = match &error.kind {
        elle::error::ErrorKind::UndefinedVariable {
            name, suggestions, ..
        } => {
            let msg = if suggestions.is_empty() {
                format!("undefined variable: {}", name)
            } else {
                format!(
                    "undefined variable: {} (did you mean: {}?)",
                    name,
                    suggestions.join(", ")
                )
            };
            ("undefined-variable", msg)
        }
        elle::error::ErrorKind::SignalMismatch {
            function,
            required_mask,
            actual_mask,
        } => (
            "signal-mismatch",
            format!(
                "function {} restricted to {} but body may emit {}",
                function, required_mask, actual_mask
            ),
        ),
        elle::error::ErrorKind::CompileError { message } => ("compile-error", message.clone()),
        elle::error::ErrorKind::SyntaxError { message, .. } => ("syntax-error", message.clone()),
        _ => ("error", error.description()),
    };
    format!(
        r#"{{"error":"compile-error","kind":"{}","file":"{}","line":{},"col":{},"message":"{}"}}"#,
        kind,
        file.replace('\\', "\\\\").replace('"', "\\\""),
        line,
        col,
        message.replace('\\', "\\\\").replace('"', "\\\""),
    )
}
