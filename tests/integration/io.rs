use crate::common::eval_source;

#[test]
fn test_io_request_predicate_false_on_int() {
    let result = eval_source("(io-request? 42)").unwrap();
    assert_eq!(result, elle::Value::bool(false));
}

#[test]
fn test_io_request_predicate_false_on_string() {
    let result = eval_source("(io-request? \"hello\")").unwrap();
    assert_eq!(result, elle::Value::bool(false));
}

#[test]
fn test_io_backend_predicate_false_on_int() {
    let result = eval_source("(io-backend? 42)").unwrap();
    assert_eq!(result, elle::Value::bool(false));
}

#[test]
fn test_stream_read_line_outside_scheduler_errors() {
    // stream/read-line yields SIG_IO, which should error at top level
    let result = eval_source("(stream/read-line (port/open \"/dev/null\" :read))");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("SIG_IO") || err.contains("outside scheduler"),
        "expected SIG_IO error, got: {}",
        err
    );
}

#[test]
fn test_stream_write_outside_scheduler_errors() {
    let result = eval_source("(stream/write (port/stdout) \"hello\")");
    assert!(result.is_err());
}

#[test]
fn test_stream_write_non_port_errors() {
    // stream/write with a non-port should signal an error
    let result = eval_source("(stream/write 42 \"hello\")");
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert!(
        err.contains("type-error") || err.contains("port"),
        "expected type-error for non-port, got: {}",
        err
    );
}

#[test]
fn test_io_backend_sync() {
    let result = eval_source("(io-backend? (io/backend :sync))").unwrap();
    assert_eq!(result, elle::Value::bool(true));
}

#[test]
fn test_io_backend_invalid_kind() {
    let result = eval_source("(io/backend :invalid)");
    assert!(result.is_err());
}

#[test]
fn test_io_execute_roundtrip() {
    // Write a file, then read it back via io/execute
    let result = eval_source(
        "(begin
           (spit \"/tmp/elle-test-io-exec\" \"hello from elle\")
           (let* ((backend (io/backend :sync))
                  (port (port/open \"/tmp/elle-test-io-exec\" :read))
                  (f (fiber/new (fn [] (stream/read-all port)) 512)))
             (fiber/resume f)
             (io/execute backend (fiber/value f))))",
    )
    .unwrap();
    result
        .with_string(|s| assert_eq!(s, "hello from elle"))
        .unwrap();
}
