// LSP server integration tests
//
// Spawns `elle lsp` as a subprocess and exercises the JSON-RPC protocol.

use serde_json::{json, Value};
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Command, Stdio};

fn get_elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// Build a JSON-RPC message with Content-Length header.
fn make_message(obj: Value) -> Vec<u8> {
    let body = serde_json::to_string(&obj).unwrap();
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    let mut buf = header.into_bytes();
    buf.extend_from_slice(body.as_bytes());
    buf
}

/// Read one JSON-RPC response from the LSP server stdout.
fn read_response(reader: &mut BufReader<std::process::ChildStdout>) -> Value {
    let mut content_length: usize = 0;

    // Read headers
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line).unwrap();
        assert!(n > 0, "unexpected EOF reading headers");

        if line == "\r\n" || line == "\n" {
            break;
        }

        if let Some(rest) = line.strip_prefix("Content-Length:") {
            content_length = rest.trim().parse::<usize>().unwrap();
        }
    }

    assert!(content_length > 0, "missing Content-Length header");

    let mut body_buf = vec![0u8; content_length];
    reader.read_exact(&mut body_buf).unwrap();
    let body_str = String::from_utf8(body_buf).unwrap();
    serde_json::from_str(&body_str).expect("invalid JSON in response")
}

/// Send a message to the LSP server.
fn send(stdin: &mut dyn Write, msg: Value) {
    let data = make_message(msg);
    stdin.write_all(&data).unwrap();
    stdin.flush().unwrap();
}

/// Start the LSP server, returning (stdin, bufreader, child).
fn start_lsp() -> (
    std::process::ChildStdin,
    BufReader<std::process::ChildStdout>,
    std::process::Child,
) {
    let mut child = Command::new(get_elle_binary())
        .arg("lsp")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to start elle lsp");

    let stdin = child.stdin.take().expect("no stdin");
    let stdout = child.stdout.take().expect("no stdout");
    (stdin, BufReader::new(stdout), child)
}

/// Send initialize, return capabilities.
fn init_lsp(stdin: &mut dyn Write, reader: &mut BufReader<std::process::ChildStdout>) -> Value {
    send(
        stdin,
        json!({
            "jsonrpc": "2.0", "id": 1, "method": "initialize",
            "params": { "capabilities": {} }
        }),
    );
    let resp = read_response(reader);
    assert_eq!(resp["id"], 1);
    resp["result"]["capabilities"].clone()
}

/// Send shutdown + exit, wait for clean exit.
fn shutdown_lsp(
    mut stdin: std::process::ChildStdin,
    reader: &mut BufReader<std::process::ChildStdout>,
    child: &mut std::process::Child,
) {
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0", "id": 999, "method": "shutdown", "params": null
        }),
    );
    let resp = read_response(reader);
    assert_eq!(resp["id"], 999);

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0", "method": "exit", "params": null
        }),
    );

    // Drop stdin to unblock server if exit handling fails
    drop(stdin);
    let status = child.wait().expect("child didn't exit");
    assert!(
        status.success(),
        "LSP server should exit cleanly after shutdown"
    );
}

/// Run the full initialize → shutdown → exit lifecycle.
#[test]
fn test_lsp_initialize_shutdown() {
    let (mut stdin, mut reader, mut child) = start_lsp();

    let caps = init_lsp(&mut stdin, &mut reader);
    assert!(caps["hoverProvider"].is_boolean());
    assert!(caps["completionProvider"].is_object());
    assert_eq!(caps["completionProvider"]["triggerCharacters"][0], "(");

    shutdown_lsp(stdin, &mut reader, &mut child);
}

/// Open a document, verify empty diagnostics, then close it.
#[test]
fn test_lsp_document_lifecycle() {
    let (mut stdin, mut reader, mut child) = start_lsp();
    let _ = init_lsp(&mut stdin, &mut reader);

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": "file:///test.lisp",
                    "languageId": "elle",
                    "version": 1,
                    "text": "(+ 1 2)"
                }
            }
        }),
    );

    let diag = read_response(&mut reader);
    assert_eq!(diag["method"], "textDocument/publishDiagnostics");
    assert_eq!(diag["params"]["uri"], "file:///test.lisp");
    assert!(diag["params"]["diagnostics"].as_array().unwrap().is_empty());

    shutdown_lsp(stdin, &mut reader, &mut child);
}

/// Open a document with a syntax error and verify the diagnostic has
/// correct 0-based line/character values (not u32::MAX from underflow).
#[test]
fn test_lsp_syntax_error_diagnostic_location() {
    let (mut stdin, mut reader, mut child) = start_lsp();
    let _ = init_lsp(&mut stdin, &mut reader);

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": "file:///bad.lisp",
                    "languageId": "elle",
                    "version": 1,
                    "text": "(((("
                }
            }
        }),
    );

    let diag = read_response(&mut reader);
    assert_eq!(diag["method"], "textDocument/publishDiagnostics");

    let diags = diag["params"]["diagnostics"].as_array().unwrap();
    assert!(!diags.is_empty(), "should have at least one diagnostic");

    let d = &diags[0];
    assert_eq!(d["code"], "E0001");

    // Verify no u32::MAX (4294967295) from underflow
    let start_line = d["range"]["start"]["line"].as_u64().unwrap();
    let start_char = d["range"]["start"]["character"].as_u64().unwrap();
    let end_line = d["range"]["end"]["line"].as_u64().unwrap();
    let end_char = d["range"]["end"]["character"].as_u64().unwrap();

    assert!(
        start_line < 1000,
        "start.line should be reasonable, got {}",
        start_line
    );
    assert!(
        start_char < 1000,
        "start.character should be reasonable, got {}",
        start_char
    );
    assert!(
        end_line < 1000,
        "end.line should be reasonable, got {}",
        end_line
    );
    assert!(
        end_char < 1000,
        "end.character should be reasonable, got {}",
        end_char
    );
    assert_eq!(start_line, 0, "error is on first line (0-based)");

    shutdown_lsp(stdin, &mut reader, &mut child);
}

/// Verify hover returns information for a known symbol.
#[test]
fn test_lsp_hover() {
    let (mut stdin, mut reader, mut child) = start_lsp();
    let _ = init_lsp(&mut stdin, &mut reader);

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": "file:///hover.lisp",
                    "languageId": "elle",
                    "version": 1,
                    "text": "(def foo 42)"
                }
            }
        }),
    );
    let _ = read_response(&mut reader); // diagnostics

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0", "id": 2,
            "method": "textDocument/hover",
            "params": {
                "textDocument": { "uri": "file:///hover.lisp" },
                "position": { "line": 0, "character": 5 }
            }
        }),
    );

    let resp = read_response(&mut reader);
    assert_eq!(resp["id"], 2);
    assert!(resp["result"].is_object(), "hover should return a result");

    shutdown_lsp(stdin, &mut reader, &mut child);
}

/// Verify completion returns items including builtins.
#[test]
fn test_lsp_completion() {
    let (mut stdin, mut reader, mut child) = start_lsp();
    let _ = init_lsp(&mut stdin, &mut reader);

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": "file:///comp.lisp",
                    "languageId": "elle",
                    "version": 1,
                    "text": "(+ 1 2)"
                }
            }
        }),
    );
    let _ = read_response(&mut reader); // diagnostics

    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0", "id": 2,
            "method": "textDocument/completion",
            "params": {
                "textDocument": { "uri": "file:///comp.lisp" },
                "position": { "line": 0, "character": 1 }
            }
        }),
    );

    let resp = read_response(&mut reader);
    assert_eq!(resp["id"], 2);
    let items = resp["result"]["items"].as_array().unwrap();
    assert!(!items.is_empty(), "completion should return items");

    let labels: Vec<&str> = items
        .iter()
        .filter_map(|i| i.get("label").and_then(|l| l.as_str()))
        .collect();
    assert!(labels.contains(&"+"), "completion should include '+'");

    shutdown_lsp(stdin, &mut reader, &mut child);
}

/// Verify didChange updates the document and produces new diagnostics.
#[test]
fn test_lsp_document_change() {
    let (mut stdin, mut reader, mut child) = start_lsp();
    let _ = init_lsp(&mut stdin, &mut reader);

    // Open clean document
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didOpen",
            "params": {
                "textDocument": {
                    "uri": "file:///change.lisp",
                    "languageId": "elle",
                    "version": 1,
                    "text": "(+ 1 2)"
                }
            }
        }),
    );
    let _ = read_response(&mut reader); // empty diagnostics

    // Change to broken content
    send(
        &mut stdin,
        json!({
            "jsonrpc": "2.0",
            "method": "textDocument/didChange",
            "params": {
                "textDocument": { "uri": "file:///change.lisp", "version": 2 },
                "contentChanges": [{ "text": "(((" }]
            }
        }),
    );

    let diag = read_response(&mut reader);
    assert_eq!(diag["method"], "textDocument/publishDiagnostics");
    let diags = diag["params"]["diagnostics"].as_array().unwrap();
    assert!(!diags.is_empty(), "broken code should produce diagnostics");

    // Verify no u32::MAX in range
    let d = &diags[0];
    let start_line = d["range"]["start"]["line"].as_u64().unwrap();
    assert!(
        start_line < 1000,
        "line should be reasonable, got {}",
        start_line
    );

    shutdown_lsp(stdin, &mut reader, &mut child);
}
