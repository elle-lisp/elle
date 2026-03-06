use crate::common::eval_source;
use elle::Value;

// --- Display ---

#[test]
fn test_port_stdin_display() {
    let result = eval_source("(string (port/stdin))").unwrap();
    result
        .with_string(|s| {
            assert_eq!(s, "#<port:stdin>");
        })
        .unwrap();
}

#[test]
fn test_port_stdout_display() {
    let result = eval_source("(string (port/stdout))").unwrap();
    result
        .with_string(|s| {
            assert_eq!(s, "#<port:stdout>");
        })
        .unwrap();
}

#[test]
fn test_port_stderr_display() {
    let result = eval_source("(string (port/stderr))").unwrap();
    result
        .with_string(|s| {
            assert_eq!(s, "#<port:stderr>");
        })
        .unwrap();
}

// --- Type predicate ---

#[test]
fn test_port_predicate_true() {
    let result = eval_source("(port? (port/stdin))").unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_port_predicate_false_int() {
    let result = eval_source("(port? 42)").unwrap();
    assert_eq!(result, Value::FALSE);
}

#[test]
fn test_port_predicate_false_string() {
    let result = eval_source("(port? \"hello\")").unwrap();
    assert_eq!(result, Value::FALSE);
}

// --- Open predicate ---

#[test]
fn test_port_open_predicate_stdin() {
    let result = eval_source("(port/open? (port/stdin))").unwrap();
    assert_eq!(result, Value::TRUE);
}

#[test]
fn test_port_open_predicate_non_port_errors() {
    let result = eval_source("(port/open? 42)");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("type-error"), "got: {}", err);
}

// --- Close ---

#[test]
fn test_port_close_non_port_errors() {
    let result = eval_source("(port/close 42)");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("type-error"), "got: {}", err);
}

#[test]
fn test_port_close_idempotent() {
    // Closing a stdio port twice should not error
    let result = eval_source(
        "(let ((p (port/stdin)))
           (port/close p)
           (port/close p))",
    )
    .unwrap();
    assert_eq!(result, Value::NIL);
}

// --- Open file port ---

#[test]
fn test_port_open_nonexistent_file() {
    let result = eval_source("(port/open \"/tmp/elle-test-nonexistent-file-474\" :read)");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("io-error"), "got: {}", err);
}

#[test]
fn test_port_open_bad_mode() {
    let result = eval_source("(port/open \"/tmp/elle-test-474\" :badmode)");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("type-error"), "got: {}", err);
}

#[test]
fn test_port_open_wrong_arg_type() {
    let result = eval_source("(port/open 42 :read)");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(err.contains("type-error"), "got: {}", err);
}

#[test]
fn test_port_open_close_lifecycle() {
    let result = eval_source(
        "(let ((p (port/open \"/tmp/elle-test-port-lifecycle-474\" :write)))
           (let ((open-before (port/open? p)))
             (port/close p)
             (let ((open-after (port/open? p)))
               (list open-before open-after))))",
    )
    .unwrap();
    // Should be (true false)
    let items = result.list_to_vec().unwrap();
    assert_eq!(items[0], Value::TRUE);
    assert_eq!(items[1], Value::FALSE);
}

#[test]
fn test_port_open_file_display() {
    let result =
        eval_source("(string (port/open \"/tmp/elle-test-port-display-474\" :write))").unwrap();
    result
        .with_string(|s| {
            assert!(s.starts_with("#<port:file"), "got: {}", s);
            assert!(s.contains(":write"), "got: {}", s);
            assert!(s.contains(":text"), "got: {}", s);
        })
        .unwrap();
}

#[test]
fn test_port_open_bytes_display() {
    let result =
        eval_source("(string (port/open-bytes \"/tmp/elle-test-port-bytes-474\" :write))").unwrap();
    result
        .with_string(|s| {
            assert!(s.starts_with("#<port:file"), "got: {}", s);
            assert!(s.contains(":binary"), "got: {}", s);
        })
        .unwrap();
}

// --- type-of ---

#[test]
fn test_port_type_of() {
    // ExternalObject type_name is "port", so type-of returns :port
    let result = eval_source("(type (port/stdin))").unwrap();
    assert_eq!(result.as_keyword_name(), Some("port"));
}

// --- with macro ---

#[test]
fn test_port_with_macro() {
    let result = eval_source(
        "(with p (port/open \"/tmp/elle-test-port-with-474\" :write) port/close
           (port/open? p))",
    )
    .unwrap();
    assert_eq!(result, Value::TRUE);
}
