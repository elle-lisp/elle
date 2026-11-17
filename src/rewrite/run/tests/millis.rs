// audited: 2026-11-17
//! Tests for the epoch 14 rewrite of real source text: a duration in milliseconds becomes a `:timeout` in seconds.
//!
//! docs/epochs.md

use super::*;

/// `body` written at epoch 13, the last epoch that took milliseconds.
fn rewritten_from_13(body: &str) -> String {
    rewritten_from(13, body)
}

#[test]
fn a_keyword_literal_is_rewritten_in_place() {
    // Only the number changes. The comment after the call is the author's.
    assert_eq!(
        rewritten_from_13("(port/read p 64 :timeout 500) # half a second\n"),
        "(port/read p 64 :timeout 0.5) # half a second\n"
    );
    assert_eq!(
        rewritten_from_13("(tcp/connect h 80 :sndbuf 4096 :timeout 5000)\n"),
        "(tcp/connect h 80 :sndbuf 4096 :timeout 5)\n"
    );
}

#[test]
fn a_keyword_nil_is_left_as_written() {
    assert_eq!(
        rewritten_from_13("(port/read p 64 :timeout nil)\n"),
        "(port/read p 64 :timeout nil)\n"
    );
}

#[test]
fn a_keyword_expression_is_wrapped_where_it_stands() {
    assert_eq!(
        rewritten_from_13("(port/read p 64 :timeout (* 2 t))\n"),
        "(port/read p 64 :timeout (if-let [ms (* 2 t)] (/ ms 1000.0) nil))\n"
    );
}

#[test]
fn a_positional_literal_becomes_a_keyword() {
    assert_eq!(
        rewritten_from_13("(sys/join h 500)\n"),
        "(sys/join h :timeout 0.5)\n"
    );
    assert_eq!(
        rewritten_from_13("(chan/select @[rx] 50)\n"),
        "(chan/select @[rx] :timeout 0.05)\n"
    );
}

#[test]
fn a_positional_nil_is_removed_with_the_space_before_it() {
    assert_eq!(rewritten_from_13("(sys/join h nil)\n"), "(sys/join h)\n");
}

#[test]
fn a_negative_literal_is_removed_where_it_meant_no_bound() {
    assert_eq!(rewritten_from_13("(io/wait b -1)\n"), "(io/wait b)\n");
    assert_eq!(rewritten_from_13("(ev/step -1)\n"), "(ev/step)\n");
}

#[test]
fn a_positional_expression_keeps_its_line() {
    // The keyword goes where the argument stood, so a call laid out one
    // argument per line keeps that layout.
    assert_eq!(
        rewritten_from_13("(os/join h\n         (form-budget))\n"),
        "(os/join h\n         :timeout (if-let [ms (form-budget)] (/ ms 1000.0) nil))\n"
    );
}

#[test]
fn a_negative_expression_means_no_bound_in_the_rewritten_file() {
    assert_eq!(
        rewritten_from_13("(ev/step (- 0 1))\n"),
        "(ev/step :timeout (if-let [ms (- 0 1)] (if (< ms 0) nil (/ ms 1000.0)) nil))\n"
    );
}

#[test]
fn a_step_with_no_argument_is_rewritten_to_poll() {
    assert_eq!(rewritten_from_13("(ev/step)\n"), "(ev/step :timeout 0)\n");
}

#[test]
fn a_nested_call_is_rewritten_once() {
    assert_eq!(
        rewritten_from_13("(chan/select (f (port/read p 1 :timeout 100)) 50)\n"),
        "(chan/select (f (port/read p 1 :timeout 0.1)) :timeout 0.05)\n"
    );
}

#[test]
fn a_quoted_call_is_left_as_written() {
    assert_eq!(
        rewritten_from_13("'(sys/join h 500)\n"),
        "'(sys/join h 500)\n"
    );
}

#[test]
fn a_head_an_older_epoch_renamed_is_rewritten_under_its_new_name() {
    assert_eq!(
        rewritten_from(3, "(stream/read-line p :timeout 500)\n"),
        "(port/read-line p :timeout 0.5)\n"
    );
}

#[test]
fn a_renamed_symbol_that_is_the_whole_expression_is_renamed_inside_the_wrap() {
    // The wrap opens where the expression starts, and so does the rename of
    // `car`. Both edits must land, the opening before the new name.
    assert_eq!(
        rewritten_from(9, "(port/read p 1 :timeout car)\n"),
        "(port/read p 1 :timeout (if-let [ms first] (/ ms 1000.0) nil))\n"
    );
}
