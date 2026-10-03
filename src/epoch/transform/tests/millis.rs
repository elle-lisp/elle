// audited: 2026-09-30
//! Tests for the epoch 14 migration: a duration in milliseconds becomes a
//! `:timeout` in seconds, one case per shape the rule rewrites.
//!
//! docs/epochs.md
//! docs/io/timeout.md

use super::super::migrate;
use crate::reader::read_syntax_all;
use crate::syntax::thread_arena;

/// `source` read, migrated from `from` to epoch 14, and printed back.
fn migrated(from: u64, source: &str) -> String {
    let arena = thread_arena();
    let mut forms = read_syntax_all(arena, source, "<test>").unwrap();
    migrate(&arena, &mut forms, from, 14).unwrap();
    forms
        .iter()
        .map(|f| f.to_string())
        .collect::<Vec<_>>()
        .join("\n")
}

/// `source` migrated from epoch 13, the last epoch that took milliseconds.
fn from_13(source: &str) -> String {
    migrated(13, source)
}

#[test]
fn a_keyword_literal_becomes_seconds() {
    assert_eq!(
        from_13("(port/read p 64 :timeout 500)"),
        "(port/read p 64 :timeout 0.5)"
    );
    // A whole number of seconds stays an integer.
    assert_eq!(
        from_13("(tcp/connect h 80 :sndbuf 4096 :timeout 5000)"),
        "(tcp/connect h 80 :sndbuf 4096 :timeout 5)"
    );
    assert_eq!(
        from_13("(port/set-options p :timeout 33)"),
        "(port/set-options p :timeout 0.033)"
    );
}

#[test]
fn a_float_literal_becomes_seconds() {
    assert_eq!(
        from_13("(port/write p b :timeout 1500.0)"),
        "(port/write p b :timeout 1.5)"
    );
}

#[test]
fn a_keyword_nil_stays_nil() {
    assert_eq!(
        from_13("(port/read p 64 :timeout nil)"),
        "(port/read p 64 :timeout nil)"
    );
}

#[test]
fn a_keyword_expression_is_divided_at_run_time() {
    assert_eq!(
        from_13("(udp/recv-from s 64 :timeout (* 2 t))"),
        "(udp/recv-from s 64 :timeout (if-let [ms (* 2 t)] (/ ms 1000.0) nil))"
    );
}

#[test]
fn a_keyword_the_call_does_not_name_is_left_alone() {
    // `:sndbuf` is a byte count, and `port/open`'s `:write` is a mode. Only
    // the value after `:timeout` is a duration.
    assert_eq!(
        from_13("(tcp/connect h 80 :sndbuf 4096)"),
        "(tcp/connect h 80 :sndbuf 4096)"
    );
    assert_eq!(
        from_13("(port/open path :write :timeout 5000)"),
        "(port/open path :write :timeout 5)"
    );
}

#[test]
fn a_positional_literal_becomes_a_timeout_in_seconds() {
    assert_eq!(from_13("(sys/join h 500)"), "(sys/join h :timeout 0.5)");
    assert_eq!(from_13("(os/join h 5000)"), "(os/join h :timeout 5)");
    assert_eq!(
        from_13("(chan/select rxs 50)"),
        "(chan/select rxs :timeout 0.05)"
    );
    assert_eq!(
        from_13("(chan/wait-ready rxs 100)"),
        "(chan/wait-ready rxs :timeout 0.1)"
    );
    assert_eq!(from_13("(io/wait b 0)"), "(io/wait b :timeout 0)");
    assert_eq!(from_13("(ev/step 10)"), "(ev/step :timeout 0.01)");
    assert_eq!(from_13("(ev/shutdown 100)"), "(ev/shutdown :timeout 0.1)");
}

#[test]
fn a_positional_nil_is_dropped() {
    assert_eq!(from_13("(sys/join h nil)"), "(sys/join h)");
    assert_eq!(from_13("(chan/select rxs nil)"), "(chan/select rxs)");
}

#[test]
fn a_negative_literal_is_dropped_where_it_meant_no_bound() {
    assert_eq!(from_13("(io/wait b -1)"), "(io/wait b)");
    assert_eq!(from_13("(ev/step -1)"), "(ev/step)");
}

#[test]
fn a_negative_literal_elsewhere_stays_a_negative_duration() {
    // These calls refused a negative duration, and a negative number of
    // seconds is refused the same way. Dropping it would turn an error into
    // a wait with no bound.
    assert_eq!(from_13("(sys/join h -1)"), "(sys/join h :timeout -0.001)");
}

#[test]
fn a_positional_expression_is_divided_at_run_time() {
    assert_eq!(
        from_13("(os/join h (form-budget))"),
        "(os/join h :timeout (if-let [ms (form-budget)] (/ ms 1000.0) nil))"
    );
}

#[test]
fn a_negative_expression_means_no_bound_where_a_negative_literal_did() {
    // The embedding documents spelled an unbounded step `(ev/step (- 0 1))`.
    assert_eq!(
        from_13("(ev/step (- 0 1))"),
        "(ev/step :timeout (if-let [ms (- 0 1)] (if (< ms 0) nil (/ ms 1000.0)) nil))"
    );
}

#[test]
fn a_call_of_another_arity_is_left_alone() {
    assert_eq!(from_13("(sys/join h)"), "(sys/join h)");
    assert_eq!(from_13("(chan/select rxs)"), "(chan/select rxs)");
    // Already in the new shape: three arguments, not two.
    assert_eq!(
        from_13("(sys/join h :timeout 5)"),
        "(sys/join h :timeout 5)"
    );
}

#[test]
fn a_step_with_no_argument_gains_timeout_zero() {
    // `(ev/step)` did not wait, and a step with no bound now does.
    assert_eq!(from_13("(ev/step)"), "(ev/step :timeout 0)");
}

#[test]
fn a_stream_alias_is_rewritten_like_its_port_call() {
    assert_eq!(
        from_13("(stream/read p 64 :timeout 500)"),
        "(stream/read p 64 :timeout 0.5)"
    );
}

#[test]
fn a_head_an_older_epoch_renamed_is_matched_by_its_old_name() {
    // Epoch 4 renamed `stream/read-line` to `port/read-line`, so a file at
    // epoch 3 reaches the rule through the rename.
    assert_eq!(
        migrated(3, "(stream/read-line p :timeout 500)"),
        "(port/read-line p :timeout 0.5)"
    );
}

#[test]
fn each_duration_is_converted_once() {
    // A rewritten call is not a millisecond call: were the pass to visit it
    // again, 0.5 would read as an expression and be divided a second time.
    assert_eq!(
        from_13("(chan/select (f (port/read p 1 :timeout 100)) 50)"),
        "(chan/select (f (port/read p 1 :timeout 0.1)) :timeout 0.05)"
    );
}

#[test]
fn a_quoted_call_is_data() {
    assert_eq!(from_13("'(sys/join h 500)"), "'(sys/join h 500)");
}

#[test]
fn a_file_at_epoch_14_is_not_migrated() {
    let arena = thread_arena();
    let mut forms = read_syntax_all(arena, "(sys/join h 500)", "<test>").unwrap();
    assert_eq!(migrate(&arena, &mut forms, 14, 14).unwrap(), 0);
    assert_eq!(forms[0].to_string(), "(sys/join h 500)");
}
