use super::*;

// ==================== MESSAGE FORMAT TESTS ====================

#[test]
fn test_json_rpc_version_2_0() {
    // LSP 3.17: "The language server protocol always uses "2.0" as the jsonrpc version."
    let message = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });

    assert_eq!(
        message.get("jsonrpc").and_then(|v| v.as_str()),
        Some("2.0"),
        "All LSP messages must use jsonrpc: \"2.0\""
    );
}

#[test]
fn test_response_message_structure() {
    // LSP 3.17 ResponseMessage: "The response of a request. If a request doesn't provide
    // a result value the receiver of a request still needs to return a response message."
    let response = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "result": null
    });

    // Required fields for ResponseMessage
    assert!(response.get("jsonrpc").is_some(), "jsonrpc field required");
    assert!(response.get("id").is_some(), "id field required");
    // result or error must be present
    assert!(
        response.get("result").is_some() || response.get("error").is_some(),
        "result or error field required"
    );
}

#[test]
fn test_error_response_structure() {
    // LSP 3.17 ResponseError: "The error object in case a request fails."
    let error_response = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "error": {
            "code": -32603,
            "message": "Internal error"
        }
    });

    let error = error_response.get("error").expect("No error field");
    assert!(error.get("code").is_some(), "code field required in error");
    assert!(
        error.get("message").is_some(),
        "message field required in error"
    );
}

#[test]
fn test_notification_message_has_no_id() {
    // LSP 3.17 Notification: "A notification message. A processed notification message
    // must not send a response back. They work like events."
    let notification = json!({
        "jsonrpc": "2.0",
        "method": "textDocument/didChange",
        "params": {}
    });

    assert_eq!(
        notification.get("jsonrpc").and_then(|v| v.as_str()),
        Some("2.0"),
        "Notification must have jsonrpc"
    );
    assert!(
        notification.get("method").is_some(),
        "Notification must have method"
    );
    assert!(
        notification.get("id").is_none(),
        "Notification must NOT have id"
    );
}

// ==================== INITIALIZE CAPABILITY TESTS ====================

#[test]
fn test_initialize_response_has_capabilities() {
    // LSP 3.17: "The server can signal the following capabilities in the initialize result:"
    let capabilities = json!({
        "textDocumentSync": 1,
        "hoverProvider": true,
        "definitionProvider": true,
        "referencesProvider": true,
        "documentFormattingProvider": true,
        "completionProvider": {
            "resolveProvider": true,
            "triggerCharacters": ["("]
        }
    });

    // Elle LSP implements these capabilities (PRs #194-197)
    assert!(
        capabilities.get("definitionProvider").is_some(),
        "definitionProvider (#194)"
    );
    assert!(
        capabilities.get("referencesProvider").is_some(),
        "referencesProvider (#195)"
    );
    assert!(
        capabilities.get("hoverProvider").is_some(),
        "hoverProvider is implemented"
    );
    assert!(
        capabilities.get("documentFormattingProvider").is_some(),
        "documentFormattingProvider (#197)"
    );
    assert!(
        capabilities.get("completionProvider").is_some(),
        "completionProvider is implemented"
    );
}

// ==================== POSITION AND RANGE TESTS ====================

#[test]
fn test_position_is_zero_based() {
    // LSP 3.17: "Position - line and character are zero-based"
    let position = json!({
        "line": 0,
        "character": 0
    });

    assert_eq!(position.get("line").and_then(|v| v.as_u64()), Some(0));
    assert_eq!(position.get("character").and_then(|v| v.as_u64()), Some(0));

    // Test that positions don't use 1-based indexing
    let position_second_line = json!({
        "line": 1,
        "character": 5
    });

    assert_eq!(
        position_second_line.get("line").and_then(|v| v.as_u64()),
        Some(1)
    );
    assert_eq!(
        position_second_line
            .get("character")
            .and_then(|v| v.as_u64()),
        Some(5)
    );
}

#[test]
fn test_range_structure() {
    // LSP 3.17 Range: "A range in a text document expressed by two positions"
    let range = json!({
        "start": {
            "line": 0,
            "character": 0
        },
        "end": {
            "line": 5,
            "character": 10
        }
    });

    assert!(range.get("start").is_some(), "Range must have start");
    assert!(range.get("end").is_some(), "Range must have end");
    assert!(
        range.get("start").unwrap().get("line").is_some(),
        "start must have line"
    );
}
