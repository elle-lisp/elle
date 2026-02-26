//! LSP server main loop and JSON-RPC dispatch.

use crate::lsp::{completion, definition, formatting, hover, references, rename, CompilerState};
use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};

/// Run the LSP server. Reads JSON-RPC from stdin, writes to stdout.
/// Returns 0 on clean shutdown.
pub fn run() -> i32 {
    let mut compiler_state = CompilerState::new();

    let stdin = std::io::stdin();
    let mut reader = BufReader::new(stdin.lock());
    let mut stdout = std::io::stdout();

    loop {
        // Read headers until Content-Length
        let mut content_length: usize = 0;
        let mut line = String::new();

        loop {
            line.clear();
            if reader.read_line(&mut line).unwrap() == 0 {
                return 0; // EOF
            }

            if line == "\r\n" || line == "\n" {
                break;
            }

            if line.starts_with("Content-Length:") {
                if let Ok(len) = line.split(':').nth(1).unwrap_or("").trim().parse::<usize>() {
                    content_length = len;
                }
            }
        }

        if content_length == 0 {
            continue;
        }

        // Read message body
        let mut buf = vec![0u8; content_length];
        if reader.read_exact(&mut buf).is_err() {
            break;
        }

        let message = String::from_utf8_lossy(&buf);
        if let Ok(request) = serde_json::from_str::<Value>(&message) {
            let (response, notifications) = handle_request(&request, &mut compiler_state);

            // Only send response for requests (not notifications)
            if request.get("id").is_some() {
                let body = response.to_string();
                let _ = write!(stdout, "Content-Length: {}\r\n\r\n{}", body.len(), body);
                let _ = stdout.flush();
            }

            // Send notifications (e.g., diagnostics)
            for notification in notifications {
                let body = notification.to_string();
                let _ = write!(stdout, "Content-Length: {}\r\n\r\n{}", body.len(), body);
                let _ = stdout.flush();
            }
        }
    }

    0
}

/// Extract the word/prefix at the given position
fn extract_prefix_at_position(text: &str, line: u32, character: u32) -> String {
    let lines: Vec<&str> = text.lines().collect();

    if line as usize >= lines.len() {
        return String::new();
    }

    let target_line = lines[line as usize];
    let col = character as usize;

    if col > target_line.len() {
        return String::new();
    }

    let mut start = col;
    for (i, ch) in target_line[..col].chars().rev().enumerate() {
        if !ch.is_alphanumeric() && ch != '-' && ch != '_' {
            start = col - i;
            break;
        }
        if i == col - 1 {
            start = 0;
        }
    }

    target_line[start..col].to_string()
}

fn handle_request(request: &Value, compiler_state: &mut CompilerState) -> (Value, Vec<Value>) {
    let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let id = request.get("id");
    let params = request.get("params");
    let mut notifications = Vec::new();

    let response = match method {
        "initialize" => {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "capabilities": {
                        "textDocumentSync": 1,
                        "hoverProvider": true,
                        "definitionProvider": true,
                        "referencesProvider": true,
                        "renameProvider": {
                            "prepareProvider": false,
                            "workspaceEdits": false
                        },
                        "documentFormattingProvider": true,
                        "completionProvider": {
                            "resolveProvider": true,
                            "triggerCharacters": ["("]
                        }
                    },
                    "serverInfo": {
                        "name": "Elle Language Server",
                        "version": "0.1.0"
                    }
                }
            })
        }
        "shutdown" => {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": null
            })
        }
        "textDocument/didOpen" => {
            if let Some(params) = params {
                if let Some(uri) = params
                    .get("textDocument")
                    .and_then(|d| d.get("uri"))
                    .and_then(|u| u.as_str())
                {
                    if let Some(text) = params
                        .get("textDocument")
                        .and_then(|d| d.get("text"))
                        .and_then(|t| t.as_str())
                    {
                        compiler_state.on_document_open(uri.to_string(), text.to_string());
                        compiler_state.compile_document(uri);

                        if let Some(doc) = compiler_state.get_document(uri) {
                            let diags: Vec<_> = doc
                                .diagnostics
                                .iter()
                                .map(|d| {
                                    let (line, col) = match &d.location {
                                        Some(loc) => (loc.line as u32, loc.col as u32),
                                        None => (0, 0),
                                    };
                                    json!({
                                        "range": {
                                            "start": { "line": line - 1, "character": col - 1 },
                                            "end": { "line": line - 1, "character": col }
                                        },
                                        "severity": match d.severity {
                                            crate::lint::diagnostics::Severity::Error => 1,
                                            crate::lint::diagnostics::Severity::Warning => 2,
                                            crate::lint::diagnostics::Severity::Info => 3,
                                        },
                                        "code": d.code,
                                        "source": "elle-lint",
                                        "message": d.message
                                    })
                                })
                                .collect();

                            notifications.push(json!({
                                "jsonrpc": "2.0",
                                "method": "textDocument/publishDiagnostics",
                                "params": {
                                    "uri": uri,
                                    "diagnostics": diags
                                }
                            }));
                        }
                    }
                }
            }
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": null
            })
        }
        "textDocument/didChange" => {
            if let Some(params) = params {
                if let Some(uri) = params
                    .get("textDocument")
                    .and_then(|d| d.get("uri"))
                    .and_then(|u| u.as_str())
                {
                    if let Some(changes) = params.get("contentChanges").and_then(|c| c.as_array()) {
                        if let Some(text) = changes
                            .first()
                            .and_then(|c| c.get("text"))
                            .and_then(|t| t.as_str())
                        {
                            compiler_state.on_document_change(uri, text.to_string());
                            compiler_state.compile_document(uri);

                            if let Some(doc) = compiler_state.get_document(uri) {
                                let diags: Vec<_> = doc
                                    .diagnostics
                                    .iter()
                                    .map(|d| {
                                        let (line, col) = match &d.location {
                                            Some(loc) => (loc.line as u32, loc.col as u32),
                                            None => (0, 0),
                                        };
                                        json!({
                                            "range": {
                                                "start": { "line": line - 1, "character": col - 1 },
                                                "end": { "line": line - 1, "character": col }
                                            },
                                        "severity": match d.severity {
                                            crate::lint::diagnostics::Severity::Error => 1,
                                            crate::lint::diagnostics::Severity::Warning => 2,
                                            crate::lint::diagnostics::Severity::Info => 3,
                                        },
                                            "code": d.code,
                                            "source": "elle-lint",
                                            "message": d.message
                                        })
                                    })
                                    .collect();

                                notifications.push(json!({
                                    "jsonrpc": "2.0",
                                    "method": "textDocument/publishDiagnostics",
                                    "params": {
                                        "uri": uri,
                                        "diagnostics": diags
                                    }
                                }));
                            }
                        }
                    }
                }
            }
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": null
            })
        }
        "textDocument/didClose" => {
            if let Some(params) = params {
                if let Some(uri) = params
                    .get("textDocument")
                    .and_then(|d| d.get("uri"))
                    .and_then(|u| u.as_str())
                {
                    compiler_state.on_document_close(uri);
                }
            }
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": null
            })
        }
        "textDocument/hover" => {
            let mut result = None;

            if let Some(params) = params {
                if let Some(uri) = params
                    .get("textDocument")
                    .and_then(|d| d.get("uri"))
                    .and_then(|u| u.as_str())
                {
                    if let Some(position) = params.get("position").and_then(|p| p.as_object()) {
                        if let (Some(line), Some(character)) = (
                            position.get("line").and_then(|l| l.as_u64()),
                            position.get("character").and_then(|c| c.as_u64()),
                        ) {
                            if let Some(doc) = compiler_state.get_document(uri) {
                                result = hover::find_hover_info(
                                    line as u32,
                                    character as u32,
                                    &doc.symbol_index,
                                    compiler_state.symbol_table(),
                                );
                            }
                        }
                    }
                }
            }

            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result
            })
        }
        "textDocument/completion" => {
            let mut items = Vec::new();

            if let Some(params) = params {
                if let Some(uri) = params
                    .get("textDocument")
                    .and_then(|d| d.get("uri"))
                    .and_then(|u| u.as_str())
                {
                    if let Some(position) = params.get("position").and_then(|p| p.as_object()) {
                        if let (Some(line), Some(character)) = (
                            position.get("line").and_then(|l| l.as_u64()),
                            position.get("character").and_then(|c| c.as_u64()),
                        ) {
                            let prefix = if let Some(doc) = compiler_state.get_document(uri) {
                                extract_prefix_at_position(
                                    &doc.source_text,
                                    line as u32,
                                    character as u32,
                                )
                            } else {
                                String::new()
                            };

                            if let Some(doc) = compiler_state.get_document(uri) {
                                items = completion::get_completions(
                                    line as u32,
                                    character as u32,
                                    &prefix,
                                    &doc.symbol_index,
                                    compiler_state.symbol_table(),
                                );
                            }
                        }
                    }
                }
            }

            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "isIncomplete": false,
                    "items": items
                }
            })
        }
        "textDocument/definition" => {
            let mut result = None;

            if let Some(params) = params {
                if let Some(uri) = params
                    .get("textDocument")
                    .and_then(|d| d.get("uri"))
                    .and_then(|u| u.as_str())
                {
                    if let Some(position) = params.get("position").and_then(|p| p.as_object()) {
                        if let (Some(line), Some(character)) = (
                            position.get("line").and_then(|l| l.as_u64()),
                            position.get("character").and_then(|c| c.as_u64()),
                        ) {
                            if let Some(doc) = compiler_state.get_document(uri) {
                                result = definition::find_definition(
                                    line as u32,
                                    character as u32,
                                    &doc.symbol_index,
                                    compiler_state.symbol_table(),
                                );
                            }
                        }
                    }
                }
            }

            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": result
            })
        }
        "textDocument/references" => {
            let mut results = Vec::new();

            if let Some(params) = params {
                if let Some(uri) = params
                    .get("textDocument")
                    .and_then(|d| d.get("uri"))
                    .and_then(|u| u.as_str())
                {
                    if let Some(position) = params.get("position").and_then(|p| p.as_object()) {
                        if let (Some(line), Some(character)) = (
                            position.get("line").and_then(|l| l.as_u64()),
                            position.get("character").and_then(|c| c.as_u64()),
                        ) {
                            let include_declaration = params
                                .get("context")
                                .and_then(|ctx| ctx.get("includeDeclaration"))
                                .and_then(|v| v.as_bool())
                                .unwrap_or(false);

                            if let Some(doc) = compiler_state.get_document(uri) {
                                results = references::find_references(
                                    line as u32,
                                    character as u32,
                                    include_declaration,
                                    &doc.symbol_index,
                                    compiler_state.symbol_table(),
                                );
                            }
                        }
                    }
                }
            }

            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": results
            })
        }
        "textDocument/formatting" => {
            let mut result = Vec::new();
            let mut error = None;

            if let Some(params) = params {
                if let Some(uri) = params
                    .get("textDocument")
                    .and_then(|d| d.get("uri"))
                    .and_then(|u| u.as_str())
                {
                    if let Some(doc) = compiler_state.get_document(uri) {
                        let (end_line, end_char) =
                            formatting::document_end_position(&doc.source_text);

                        match formatting::format_document(&doc.source_text, end_line, end_char) {
                            Ok(edits) => result = edits,
                            Err(e) => {
                                error = Some(json!({
                                    "code": -32603,
                                    "message": format!("Formatting error: {}", e)
                                }));
                            }
                        }
                    }
                }
            }

            if let Some(err) = error {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": err
                })
            } else {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result
                })
            }
        }
        "textDocument/rename" => {
            let mut result = None;
            let mut error = None;

            if let Some(params) = params {
                if let Some(uri) = params
                    .get("textDocument")
                    .and_then(|d| d.get("uri"))
                    .and_then(|u| u.as_str())
                {
                    if let Some(position) = params.get("position").and_then(|p| p.as_object()) {
                        if let (Some(line), Some(character)) = (
                            position.get("line").and_then(|l| l.as_u64()),
                            position.get("character").and_then(|c| c.as_u64()),
                        ) {
                            if let Some(new_name) = params.get("newName").and_then(|n| n.as_str()) {
                                if let Some(doc) = compiler_state.get_document(uri) {
                                    match rename::rename_symbol(
                                        line as u32,
                                        character as u32,
                                        new_name,
                                        &doc.symbol_index,
                                        compiler_state.symbol_table(),
                                        &doc.source_text,
                                        uri,
                                    ) {
                                        Ok(workspace_edit) => result = Some(workspace_edit),
                                        Err(e) => {
                                            error = Some(json!({
                                                "code": -32603,
                                                "message": e
                                            }));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            if let Some(err) = error {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "error": err
                })
            } else {
                json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": result
                })
            }
        }
        _ => {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": null
            })
        }
    };

    (response, notifications)
}
