use crate::common::{eval_source, eval_source_unscheduled};

// An I/O primitive raises `:io` and nothing else names the scheduler round
// trip, so both tests below read the reported keyword rather than the fact of
// failure. The counter-factual: assert only `is_err()`, and the two pass
// unchanged when the request arrives at the root as an unreadable bitmask.

#[test]
fn test_stream_write_outside_scheduler_errors() {
    // Run WITHOUT a scheduler (eval_source wraps in ev/run) so the request has
    // nothing to service it and reaches the root.
    eval_source_unscheduled("(port/write (port/stdout) \"hello\")", |result| {
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains(":io"),
            "an unserviced port/write must report :io, got: {}",
            err
        );
    });
}

#[test]
fn test_stream_read_line_outside_scheduler_errors() {
    eval_source_unscheduled("(port/read-line (port/open \"/dev/null\" :read))", |result| {
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(
            err.contains(":io"),
            "an unserviced port/read-line must report :io, got: {}",
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
