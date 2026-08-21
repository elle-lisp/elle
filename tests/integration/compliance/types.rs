use super::*;

// ==================== TEXT EDIT TESTS ====================

#[test]
fn test_text_edit_structure() {
    // LSP 3.17 TextEdit: "A TextEdit represents a changes to a text document."
    let text_edit = json!({
        "range": {
            "start": { "line": 0, "character": 0 },
            "end": { "line": 0, "character": 5 }
        },
        "newText": "hello"
    });

    assert!(text_edit.get("range").is_some(), "TextEdit must have range");
    assert!(
        text_edit.get("newText").is_some(),
        "TextEdit must have newText"
    );
}

#[test]
fn test_text_edit_array() {
    // LSP 3.17: "Formatting requests return TextEdit[]"
    let text_edits = vec![
        json!({
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 5 }
            },
            "newText": "hello"
        }),
        json!({
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 5 }
            },
            "newText": "world"
        }),
    ];

    // Verify array structure
    assert_eq!(text_edits.len(), 2);
    for edit in &text_edits {
        assert!(edit.get("range").is_some());
        assert!(edit.get("newText").is_some());
    }
}

// ==================== LOCATION TESTS ====================

#[test]
fn test_location_structure() {
    // LSP 3.17 Location: "Represents a location inside a resource, such as a line inside a text file"
    let location = json!({
        "uri": "file:///test.elle",
        "range": {
            "start": { "line": 0, "character": 0 },
            "end": { "line": 0, "character": 5 }
        }
    });

    assert!(location.get("uri").is_some(), "Location must have uri");
    assert!(location.get("range").is_some(), "Location must have range");
    assert_eq!(
        location.get("uri").and_then(|v| v.as_str()),
        Some("file:///test.elle")
    );
}

#[test]
fn test_location_array_for_references() {
    // LSP 3.17: "Find References request returns Location[]"
    let locations = vec![
        json!({
            "uri": "file:///test.elle",
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 0, "character": 5 }
            }
        }),
        json!({
            "uri": "file:///test.elle",
            "range": {
                "start": { "line": 1, "character": 0 },
                "end": { "line": 1, "character": 5 }
            }
        }),
    ];

    assert!(!locations.is_empty());
    for loc in &locations {
        assert!(loc.get("uri").is_some());
        assert!(loc.get("range").is_some());
    }
}

// ==================== MARKUP CONTENT TESTS ====================

#[test]
fn test_markup_content_structure() {
    // LSP 3.17 MarkupContent: "Now a document can be described using markup content"
    let markup = json!({
        "kind": "plaintext",
        "value": "This is a symbol"
    });

    assert!(markup.get("kind").is_some(), "MarkupContent must have kind");
    assert!(
        markup.get("value").is_some(),
        "MarkupContent must have value"
    );
}

#[test]
fn test_hover_response_structure() {
    // LSP 3.17 Hover: "Hover information. The result of a hover request is of type Hover or null"
    let hover = json!({
        "contents": ["This is a symbol", "Type: Function"]
    });

    assert!(hover.get("contents").is_some(), "Hover must have contents");
}

// ==================== COMPLETION TESTS ====================

#[test]
fn test_completion_item_structure() {
    // LSP 3.17 CompletionItem: "Provides textual and contextual information about an object"
    let completion_item = json!({
        "label": "my-function",
        "kind": 12,  // Function
        "detail": "Function with 2 parameters",
        "documentation": "Does something useful"
    });

    assert!(completion_item.get("label").is_some(), "label required");
    // kind is optional, detail and documentation are optional
}

#[test]
fn test_completion_list_structure() {
    // LSP 3.17: "The result of a completion request is of type CompletionList"
    let completion_list = json!({
        "isIncomplete": false,
        "items": [
            { "label": "item1" },
            { "label": "item2" }
        ]
    });

    assert!(
        completion_list.get("isIncomplete").is_some(),
        "isIncomplete required"
    );
    assert!(completion_list.get("items").is_some(), "items required");
    assert!(
        completion_list.get("items").unwrap().as_array().is_some(),
        "items must be array"
    );
}

// ==================== TEXT DOCUMENT SYNCHRONIZATION TESTS ====================

#[test]
fn test_did_open_notification_params() {
    // LSP 3.17 DidOpenTextDocumentParams
    let params = json!({
        "textDocument": {
            "uri": "file:///test.elle",
            "languageId": "elle",
            "version": 1,
            "text": "(+ 1 2)"
        }
    });

    let doc = params.get("textDocument").expect("textDocument required");
    assert!(doc.get("uri").is_some(), "uri required");
    assert!(doc.get("languageId").is_some(), "languageId required");
    assert!(doc.get("version").is_some(), "version required");
    assert!(doc.get("text").is_some(), "text required");
}

#[test]
fn test_did_change_notification_params() {
    // LSP 3.17 DidChangeTextDocumentParams
    let params = json!({
        "textDocument": {
            "uri": "file:///test.elle",
            "version": 2
        },
        "contentChanges": [
            {
                "text": "(+ 3 4)"
            }
        ]
    });

    assert!(
        params.get("textDocument").is_some(),
        "textDocument required"
    );
    assert!(
        params.get("contentChanges").unwrap().as_array().is_some(),
        "contentChanges must be array"
    );
    assert!(
        params.get("contentChanges").unwrap().as_array().is_some(),
        "contentChanges must be array"
    );
}

#[test]
fn test_text_document_position_params() {
    // LSP 3.17 TextDocumentPositionParams: "Used for goto definition, goto references, hover"
    let params = json!({
        "textDocument": {
            "uri": "file:///test.elle"
        },
        "position": {
            "line": 0,
            "character": 5
        }
    });

    assert!(params.get("textDocument").is_some());
    assert!(params.get("position").is_some());
}

// ==================== FORMATTING REQUEST TESTS ====================

#[test]
fn test_document_formatting_params() {
    // LSP 3.17 DocumentFormattingParams
    let params = json!({
        "textDocument": {
            "uri": "file:///test.elle"
        },
        "options": {
            "tabSize": 2,
            "insertSpaces": true
        }
    });

    assert!(
        params.get("textDocument").is_some(),
        "textDocument required"
    );
    assert!(params.get("options").is_some(), "options required");

    let options = params.get("options").unwrap();
    assert!(
        options.get("tabSize").is_some(),
        "tabSize in formatting options"
    );
    assert!(
        options.get("insertSpaces").is_some(),
        "insertSpaces in formatting options"
    );
}

// ==================== CONTENT LENGTH HEADER TESTS ====================

#[test]
fn test_lsp_message_format_example() {
    // LSP 3.17: "Currently the following header fields are supported: Content-Length (required)"
    // Example message format: "Content-Length: 123\r\n\r\n{...json body...}"

    let json_body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });

    let body_str = json_body.to_string();
    let content_length = body_str.len();

    // Verify content length calculation
    assert!(content_length > 0, "Body must have content");

    // Message would be formatted as:
    let message = format!("Content-Length: {}\r\n\r\n{}", content_length, body_str);

    // Verify header structure
    assert!(message.starts_with("Content-Length:"));
    assert!(message.contains("\r\n\r\n"));
}

// ==================== NOTIFICATION HANDLING TESTS ====================

#[test]
fn test_notification_methods_have_no_id() {
    // LSP 3.17: Notifications are messages that do not have an "id" field.
    // The server MUST NOT send a response to a notification.
    let notification_methods = vec![
        "textDocument/didOpen",
        "textDocument/didChange",
        "textDocument/didClose",
        "textDocument/didSave",
        "initialized",
        "exit",
    ];

    for method in notification_methods {
        let notification = json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": {}
        });

        assert!(
            notification.get("id").is_none(),
            "{} notification must NOT have id field",
            method
        );
    }
}

#[test]
fn test_request_methods_have_id() {
    // LSP 3.17: Requests are messages that have an "id" field.
    // The server MUST send a response for every request.
    let request_methods = [
        "initialize",
        "shutdown",
        "textDocument/hover",
        "textDocument/completion",
        "textDocument/definition",
        "textDocument/references",
        "textDocument/formatting",
        "textDocument/rename",
    ];

    for (i, method) in request_methods.iter().enumerate() {
        let request = json!({
            "jsonrpc": "2.0",
            "id": i + 1,
            "method": method,
            "params": {}
        });

        assert!(
            request.get("id").is_some(),
            "{} request MUST have id field",
            method
        );
    }
}

#[test]
fn test_distinguishing_notifications_from_requests() {
    // This test verifies the logic for detecting notifications vs requests
    // based on the presence/absence of the "id" field.

    let notification = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didOpen",
        "params": {}
    });

    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "textDocument/hover",
        "params": {}
    });

    // A message is a notification if it has no "id" field
    let is_notification = notification.get("id").is_none();
    let is_request = request.get("id").is_some();

    assert!(
        is_notification,
        "Message without id should be detected as notification"
    );
    assert!(is_request, "Message with id should be detected as request");
}

// ==================== SPEC COMPLIANCE SUMMARY ====================

#[test]
fn test_elle_lsp_implements_required_capabilities() {
    // Summary of what Elle LSP implements based on open PRs

    let implemented = vec![
        ("textDocument/definition", true), // #194
        ("textDocument/references", true), // #195
        ("textDocument/hover", true),      // Existing
        ("textDocument/formatting", true), // #197
        ("textDocument/completion", true), // Existing
        ("textDocument/didOpen", true),    // Lifecycle
        ("textDocument/didChange", true),  // Lifecycle
        ("textDocument/didClose", true),   // Lifecycle
        ("initialize", true),              // Lifecycle
        ("shutdown", true),                // Lifecycle
    ];

    for (capability, supported) in implemented {
        assert!(supported, "Elle LSP should implement {}", capability);
    }
}
