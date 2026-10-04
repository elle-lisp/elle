// audited: 2026-09-29
// Where an io request goes when no scheduler serves it, and when a host hands it on.
//
// docs/impl/region/park.md

use crate::common::{eval_source, eval_source_unscheduled};

// An I/O primitive raises `:io` and nothing else names the scheduler round
// trip, so the two tests below read the reported keyword rather than the fact
// of failure. The counter-factual: assert only `is_err()`, and the two pass
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

// `arena/allocs` and `compile/run-on :bytecode` run a thunk on the current
// fiber and park the thunk's io request again at their own call. The fiber that
// stamped the request at the thunk's park stamps it again at the host's park.
// The counter-factual: a stamp that admits one park per request panics at the
// host's park in a debug build, before the scheduler ever sees the request.
#[test]
fn a_host_hands_its_thunks_io_park_to_the_scheduler() {
    for host in [
        "(arena/allocs (fn [] (ev/sleep 0)))",
        "(compile/run-on :bytecode (fn [] (ev/sleep 0)))",
    ] {
        eval_source(&format!("(begin {host} :served)"), |result| {
            assert_eq!(
                result.unwrap(),
                elle::Value::keyword("served"),
                "the scheduler serves the request {host} hands on",
            );
        });
    }
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
