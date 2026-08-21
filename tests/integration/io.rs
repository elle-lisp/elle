use crate::common::{eval_source, eval_source_unscheduled};

#[test]
fn test_stream_write_outside_scheduler_errors() {
    // port/write yields SIG_IO, which can't be caught by protect in Elle.
    // Run WITHOUT a scheduler (eval_source now wraps in ev/run) so the yield
    // has nothing to service it and errors at top level.
    eval_source_unscheduled("(port/write (port/stdout) \"hello\")", |result| {
        assert!(result.is_err());
    });
}

#[test]
fn test_stream_read_line_outside_scheduler_errors() {
    // port/read-line yields SIG_IO, which should error at top level (no scheduler).
    eval_source_unscheduled("(port/read-line (port/open \"/dev/null\" :read))", |result| {
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("SIG_IO") || err.contains("outside scheduler") || err.contains("yield"),
            "expected SIG_IO error, got: {}",
            err
        );
    });
}

#[test]
fn test_stream_write_non_port_errors() {
    // port/write with a non-port should signal an error
    eval_source("(port/write 42 \"hello\")", |result| {
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains("type-error") || err.contains("port"),
            "expected type-error for non-port, got: {}",
            err
        );
    });
}
