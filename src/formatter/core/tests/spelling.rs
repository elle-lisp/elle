// audited: 2026-11-24
//! A quoting form keeps the spelling its source wrote: the prefix or the list.
//!
//! docs/impl/syntax.md
//!
//! The counter-factual: the reader builds one node for `'x` and for
//! `(quote x)`, so a formatter that printed from the node alone would
//! rewrite every written list as its prefix.

use super::*;

#[test]
fn a_quoting_form_keeps_the_spelling_its_source_wrote() {
    for text in [
        "(quote x)\n",
        "'x\n",
        "(quasiquote (a (unquote b) (unquote-splicing c)))\n",
        "`(a ,b ,;c)\n",
        "(f '(quote y) `(unquote z))\n",
    ] {
        assert_formats(text, text);
    }
}
