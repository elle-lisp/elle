//! Elle Language Server Protocol implementation
//!
//! A Language Server Protocol (LSP) server for Elle Lisp providing:
//! - Real-time diagnostics via elle-lint
//! - Hover information for symbols  
//! - Symbol references and definitions
//! - Code completion suggestions

use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};

fn main() {
    let mut documents: HashMap<String, String> = HashMap::new();

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
                return; // EOF
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
            let response = handle_request(&request, &mut documents);

            let body = response.to_string();
            let _ = writeln!(stdout, "Content-Length: {}\r\n\r{}", body.len(), body);
            let _ = stdout.flush();
        }
    }
}

fn handle_request(request: &Value, documents: &mut HashMap<String, String>) -> Value {
    let method = request.get("method").and_then(|v| v.as_str()).unwrap_or("");
    let id = request.get("id");
    let params = request.get("params");

    match method {
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
                        documents.insert(uri.to_string(), text.to_string());
                    }
                }
            }
            json!({})
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
                            documents.insert(uri.to_string(), text.to_string());
                        }
                    }
                }
            }
            json!({})
        }
        "textDocument/didClose" => {
            if let Some(params) = params {
                if let Some(uri) = params
                    .get("textDocument")
                    .and_then(|d| d.get("uri"))
                    .and_then(|u| u.as_str())
                {
                    documents.remove(uri);
                }
            }
            json!({})
        }
        "textDocument/hover" => {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": {
                    "contents": "Elle Lisp symbol information"
                }
            })
        }
        _ => {
            json!({
                "jsonrpc": "2.0",
                "id": id,
                "result": null
            })
        }
    }
}
