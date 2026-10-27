// audited: 2026-10-27
//! The prelude's body macros keep their header beside the head and break
//! their body +2.
//!
//! docs/fmt.md
//!
//! The counter-factual: a macro the formatter has no row for takes the call
//! layout, which fills its body forms onto the header's line and aligns the
//! rest under the header.

use super::*;

#[test]
fn each_body_macro_keeps_its_header_beside_the_head_and_breaks_its_body() {
    for (input, expected) in [
        (
            "(unless done (p 1) (p 2))",
            "(unless done\n  (p 1)\n  (p 2))\n",
        ),
        (
            "(when-let [x (f)] (p x) x)",
            "(when-let [x (f)]\n  (p x)\n  x)\n",
        ),
        (
            "(when-ok [x (f)] (p x) x)",
            "(when-ok [x (f)]\n  (p x)\n  x)\n",
        ),
        (
            "(with-temp-dir dir (p dir) dir)",
            "(with-temp-dir dir\n  (p dir)\n  dir)\n",
        ),
        (
            "(with-allocator alloc (p alloc) alloc)",
            "(with-allocator alloc\n  (p alloc)\n  alloc)\n",
        ),
        (
            "(ffi/with-stack [[p :int 42] [buf 64]] (use p buf) p)",
            "(ffi/with-stack [[p :int 42] [buf 64]]\n  (use p buf)\n  p)\n",
        ),
        ("(repeat 3 (p 1) (p 2))", "(repeat 3\n  (p 1)\n  (p 2))\n"),
        (
            "(gate! (ready?) \"not ready\" (p 1) :open)",
            "(gate! (ready?) \"not ready\"\n  (p 1)\n  :open)\n",
        ),
        (
            "(with conn (open-conn) close-conn (p conn) conn)",
            "(with conn (open-conn) close-conn\n  (p conn)\n  conn)\n",
        ),
    ] {
        assert_formats(input, expected);
    }
}

#[test]
fn the_reported_when_let_keeps_each_body_form_on_its_own_line() {
    let src = "(when-let [start (normal (under base (r :written)))]
  (var at start)
  (var hops 0)
  (when (> hops 0) (break :found [base at])))
";
    assert_formats(src, src);
}

#[test]
fn a_single_trivial_body_goes_inline_when_the_form_fits() {
    for src in [
        "(repeat 3 (p 1))\n",
        "(with-temp-dir dir (p dir))\n",
        "(when-let [x (f)] (p x))\n",
    ] {
        assert_formats(src, src);
    }
}

#[test]
fn a_single_trivial_body_breaks_when_the_form_does_not_fit() {
    assert_formats(
        "(with-temp-dir scratch-directory (file/write (path/join scratch-directory \"x\") \"some long content\"))",
        "(with-temp-dir scratch-directory
  (file/write (path/join scratch-directory \"x\") \"some long content\"))
",
    );
}

#[test]
fn a_bindings_vector_in_a_header_takes_the_let_layout() {
    assert_formats(
        "(when-let [a (f) b (g)] (p a b))",
        "(when-let [a (f)\n           b (g)]\n  (p a b))\n",
    );
}

#[test]
fn if_let_lays_out_its_branches_as_if_does() {
    assert_formats("(if-let [x (f)] x 0)", "(if-let [x (f)] x 0)\n");
    assert_formats(
        "(if-let [x (f)] (begin (p x) (q (r (s x)))) 0)",
        "(if-let [x (f)]\n  (begin\n    (p x)\n    (q (r (s x))))\n  0)\n",
    );
    assert_formats(
        "(if-let [a (f) b (g)] (+ a b) 0)",
        "(if-let [a (f)\n         b (g)]\n  (+ a b)\n  0)\n",
    );
}

#[test]
fn a_body_macro_with_too_few_arguments_keeps_the_call_layout() {
    assert_formats("(with conn ctor)", "(with conn ctor)\n");
}

#[test]
fn a_comment_in_a_body_macro_header_indents_two_from_the_open_paren() {
    let src = "(with-temp-dir\n  # c\n  dir\n  (p dir))\n";
    assert_formats(src, src);
}
