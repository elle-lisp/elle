// audited: 2026-10-27
//! Every example in the formatter rules is the formatter's own output.
//!
//! docs/fmt.md
//!
//! The counter-factual: a rule changes and its example does not. The build
//! runs the examples as a program, and a stale layout runs as well as a
//! fresh one, so only formatting the example again shows it.

use super::*;

const RULES: &str = include_str!("../../../../docs/fmt.md");

/// The body of each ```lisp fence in `text`.
fn lisp_fences(text: &str) -> Vec<String> {
    let mut fences = Vec::new();
    let mut open: Option<String> = None;
    for line in text.lines() {
        match open.as_mut() {
            None if line == "```lisp" => open = Some(String::new()),
            None => {}
            Some(_) if line == "```" => fences.extend(open.take()),
            Some(body) => {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    fences
}

#[test]
fn every_example_in_the_rules_is_the_formatters_output() {
    let fences = lisp_fences(RULES);
    assert!(fences.len() >= 19, "found {} examples", fences.len());
    for fence in fences {
        assert_formats(&fence, &fence);
    }
}
