use crate::common::eval_source;

#[test]
fn test_stream_write_outside_scheduler_errors() {
    // stream/write yields SIG_IO, which can't be caught by protect in Elle
    let result = eval_source("(stream/write (port/stdout) \"hello\")");
    assert!(result.is_err());
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
