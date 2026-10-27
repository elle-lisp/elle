// audited: 2026-10-27
//! Where a long form wraps: a call with no room after its name, a form that
//! opens past half the line width, and a pair body too long for its test's
//! line.
//!
//! docs/fmt.md
//!
//! The counter-factual: past column 40 the renderer kept the enclosing
//! indent, so a wrapped argument or body started at or left of its own paren.
//! The output is idempotent, so no idempotency test sees it.

use super::*;

#[test]
fn a_call_with_no_room_after_its_name_wraps_two_from_its_paren() {
    assert_formats(
        "(defn f [path r slash exists]
  (let [a 1]
    (let [b 2]
      (let [c 3]
        (let [d 4]
          (let [e 5
                spelling (when (array? found) (spell:spelled path (r :form) (found 1) (found 0) slash exists))]
            spelling))))))",
        "(defn f [path r slash exists]
  (let [a 1]
    (let [b 2]
      (let [c 3]
        (let [d 4]
          (let [e 5
                spelling (when (array? found)
                           (spell:spelled path (r :form) (found 1) (found 0)
                             slash exists))]
            spelling))))))
",
    );
}

#[test]
fn a_first_argument_that_does_not_fit_beside_a_far_head_starts_the_next_line() {
    assert_formats(
        "(def x
  (let [the-worker-process (process:spawn-link (fn [] (process:recv) (error {:error :boom :message \"crash\"})))]
    the-worker-process))",
        "(def x
  (let [the-worker-process (process:spawn-link
                             (fn []
                               (process:recv)
                               (error {:error :boom :message \"crash\"})))]
    the-worker-process))
",
    );
}

#[test]
fn a_comment_above_the_first_argument_of_a_far_head_indents_two_from_its_paren() {
    let src = "(let [a-binding-name-long-enough-to-push (some-call
                                           # c
                                           a b)]
  a-binding-name-long-enough-to-push)
";
    assert_formats(src, src);
}

#[test]
fn the_arguments_after_a_comment_on_a_far_head_indent_two_from_its_paren() {
    let src = "(let [a-binding-name-long-enough-to-push (some-call  # c
                                           a b)]
  a-binding-name-long-enough-to-push)
";
    assert_formats(src, src);
}

#[test]
fn a_form_that_opens_past_half_the_line_indents_from_its_own_paren() {
    assert_formats(
        "(let [on-disconnect-callback-for-the-worker (fn [x] (log x) (stop x))]
  on-disconnect-callback-for-the-worker)",
        "(let [on-disconnect-callback-for-the-worker (fn [x]
                                              (log x)
                                              (stop x))]
  on-disconnect-callback-for-the-worker)
",
    );
}

#[test]
fn a_cond_body_too_long_for_its_tests_line_breaks_below_the_test() {
    assert_formats(
        "(cond
  (or (= old new) (= old \"\") (= new \"\")) (refuse (string \"`\" old \"` onto `\" new \"` is not a move\"))
  nil)",
        "(cond
  (or (= old new) (= old \"\") (= new \"\"))
    (refuse (string \"`\" old \"` onto `\" new \"` is not a move\"))
  nil)
",
    );
}

#[test]
fn a_match_body_too_long_for_its_patterns_line_breaks_below_the_pattern() {
    assert_formats(
        "(match msg
  [:EXIT pid reason] (assert (= pid worker-a) \"link-cascade: EXIT from worker-a\")
  _ :other)",
        "(match msg
  [:EXIT pid reason]
    (assert (= pid worker-a) \"link-cascade: EXIT from worker-a\")
  _ :other)
",
    );
}

#[test]
fn a_trivial_cond_body_with_an_inline_comment_stays_beside_its_test() {
    let src = "(cond\n  t :a  # c\n  :b)\n";
    assert_formats(src, src);
}
