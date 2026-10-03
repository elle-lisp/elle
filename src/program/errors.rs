// audited: 2026-09-29
//! How a failed run reads: a runtime error with its names resolved, and a compile error as text or JSON.
//!
//! docs/config.md

/// Format a runtime error, naming any `SymbolId(N)` it carries through
/// `symbols` — the instance that raised the error, and so the only memo that
/// can hold the name (docs/impl/symbol.md).
pub(super) fn format_runtime_error(error: &str, symbols: &crate::symbol::SymbolTable) -> String {
    // Check for SymbolId pattern and resolve it
    if let Some(start) = error.find("SymbolId(") {
        if let Some(end) = error[start..].find(')') {
            let id_str = &error[start + 9..start + end];
            if let Ok(id) = id_str.parse::<u64>() {
                let name = symbols
                    .name(crate::value::SymbolId(id))
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
pub(super) fn parse_compilation_error(error: &str) -> crate::error::LError {
    if let Some((file, line, col, message)) = crate::error::parse_located_error(error) {
        crate::error::LError::new(crate::error::ErrorKind::CompileError {
            message: message.to_string(),
        })
        .with_location(crate::error::SourceLoc::new(file, line, col))
    } else {
        crate::error::LError::compile_error(error)
    }
}

/// Format a compilation error as JSON for --json mode
pub(super) fn format_error_json(error: &crate::error::LError) -> String {
    let (file, line, col) = match &error.location {
        Some(loc) => (loc.file.as_str(), loc.line, loc.col),
        None => (crate::reader::UNKNOWN_FILE, 0, 0),
    };
    let (kind, message) = match &error.kind {
        crate::error::ErrorKind::UndefinedVariable {
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
        crate::error::ErrorKind::SignalMismatch {
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
        crate::error::ErrorKind::CompileError { message } => ("compile-error", message.clone()),
        crate::error::ErrorKind::SyntaxError { message, .. } => ("syntax-error", message.clone()),
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
