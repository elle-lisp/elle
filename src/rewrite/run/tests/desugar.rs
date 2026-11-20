// audited: 2026-11-18
//! Tests for the shorthand desugar pass of `elle rewrite`: a shorthand a rule names becomes the form it stands for.
//!
//! docs/impl/lexicon.md

use super::*;

/// `source` read under the current lexicon, with `shorthands` spelled out.
///
/// No epoch declares a `Desugar` rule yet, so these tests name the shorthand
/// directly rather than reach the pass through the migration table.
fn desugared(source: &str, shorthands: &[Token<'static>]) -> String {
    let edits = collect_desugar_edits(read_under(source, Lexicon::current()), shorthands).unwrap();
    applied(source, edits)
}

/// The rule an epoch that takes `;` away from the splice would carry:
/// `;x` → `(splice x)`.
fn splice_only() -> Vec<Token<'static>> {
    vec![Token::Splice]
}

#[test]
fn a_shorthand_becomes_the_form_it_stands_for() {
    assert_eq!(
        desugared("(f ;args)\n", &splice_only()),
        "(f (splice args))\n"
    );
}

#[test]
fn a_shorthand_over_a_compound_form_wraps_the_whole_form() {
    assert_eq!(
        desugared("(f ;(rest xs))\n", &splice_only()),
        "(f (splice (rest xs)))\n"
    );
}

#[test]
fn a_shorthand_over_a_set_literal_wraps_to_the_closing_pipe() {
    // A `|...|` set has no bracket depth to count; the walk finds its end by
    // scanning for the closing `|`. A rewrite that stopped at the opening one
    // would produce `(splice |)1 2|`, which still lexes.
    assert_eq!(
        desugared("(f ;|1 2|)\n", &splice_only()),
        "(f (splice |1 2|))\n"
    );
}

#[test]
fn whitespace_between_a_shorthand_and_its_form_is_absorbed() {
    // `;` and the form it wraps are separate tokens, so the author may leave
    // a gap. Replacing only the `;` bytes would emit `(splice  args)` here
    // and `(spliceargs)` for the gapless spelling. One of the two has to be
    // wrong, so the edit runs from the prefix to the start of the form.
    assert_eq!(
        desugared("(f ; args)\n", &splice_only()),
        "(f (splice args))\n"
    );
}

#[test]
fn a_shorthand_inside_another_shorthands_form_is_desugared_too() {
    // The trap in the one-span spelling: an edit covering all of `;(g ;xs)`
    // builds its replacement from source text, so the inner `;xs` rides
    // along unrewritten and the file keeps a spelling the epoch removed.
    assert_eq!(
        desugared("(f ;(g ;xs))\n", &splice_only()),
        "(f (splice (g (splice xs))))\n"
    );
}

#[test]
fn only_the_shorthands_the_rules_name_are_desugared() {
    // `'x` is a shorthand too, and an epoch that removed `;` did not remove
    // it. A pass keyed on "is a prefix token" would rewrite both.
    assert_eq!(
        desugared("(f 'x ;ys)\n", &splice_only()),
        "(f 'x (splice ys))\n"
    );
}

#[test]
fn the_pass_reads_each_shorthands_form_from_the_token() {
    // Nothing about the pass is specific to splice: name `'` and it spells
    // out `(quote x)`, because that is what the reader builds from it.
    assert_eq!(desugared("(f 'x)\n", &[Token::Quote]), "(f (quote x))\n");
}

#[test]
fn a_shorthand_with_no_form_after_it_is_left_alone() {
    // `(f ;)` does not parse. The rewriter has nothing to wrap and must not
    // invent an extent; the reader reports the real error. The closing paren
    // is the trap: it is a token, so a walk that only checked "is there a
    // next token" would wrap it and emit `(f (splice ))`.
    let source = "(f ;)\n";
    let edits =
        collect_desugar_edits(read_under(source, Lexicon::current()), &splice_only()).unwrap();
    assert!(edits.is_empty());
}

#[test]
fn a_shorthand_at_end_of_input_is_left_alone() {
    // Nothing follows at all. The walk must notice before it indexes past
    // the end of the token stream.
    let source = "(f ;";
    let edits =
        collect_desugar_edits(read_under(source, Lexicon::current()), &splice_only()).unwrap();
    assert!(edits.is_empty());
}

#[test]
fn a_shorthand_a_desugar_rule_owns_is_not_refused_by_the_respelling() {
    // The two passes divide the same token. `respell` refuses `;` because the
    // target lexicon has no bytes for it. That is right when nothing else
    // handles it, and wrong once a `Desugar` rule does: the refusal would
    // abort the rewrite before the desugar pass could run at all.
    let source = "(f ;xs)\n";
    let edits = collect_lexical_edits(
        read_under(source, Lexicon::current()),
        Lexicon::divergent(),
        &splice_only(),
    )
    .unwrap();
    assert!(edits.is_empty());
}

#[test]
fn a_source_without_the_shorthand_needs_no_edits() {
    let source = "(f args)\n";
    let edits =
        collect_desugar_edits(read_under(source, Lexicon::current()), &splice_only()).unwrap();
    assert!(edits.is_empty());
}

#[test]
fn a_shebang_does_not_shift_the_edits_below_it() {
    // Every offset the pass emits indexes the original bytes, shebang
    // included. The line itself is the operating system's and stays put.
    assert_eq!(
        desugared("#!/usr/bin/env elle\n(f ;xs)\n", &splice_only()),
        "#!/usr/bin/env elle\n(f (splice xs))\n"
    );
}
