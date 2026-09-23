// audited: 2026-09-23
//! Tests for `elle rewrite`: the edits each rule kind makes to real source text,
//! and the epoch tag the tool leaves behind.
//!
//! docs/epochs.md
//! docs/impl/lexicon.md

use super::*;

#[test]
fn test_rewrite_preserves_shebang() {
    let source = "#!/usr/bin/env elle\n(elle/epoch 0)\n(assert-true x \"test\")\n";
    let result = rewrite_file(source, "<test>").unwrap();
    assert!(result.is_some(), "expected rewrites to be applied");
    let (new_source, _count) = result.unwrap();
    let epoch_line = format!("(elle/epoch {})\n", CURRENT_EPOCH);
    let expected_prefix = format!("#!/usr/bin/env elle\n{}", epoch_line);
    assert!(
        new_source.starts_with(&expected_prefix),
        "shebang then epoch tag expected, got: {:?}",
        &new_source[..new_source.len().min(80)]
    );
    // Old epoch tag must not survive
    assert!(
        !new_source.contains("(elle/epoch 0)"),
        "old epoch tag should be removed"
    );
    let epoch_count = new_source.matches("elle/epoch").count();
    assert_eq!(
        epoch_count, 1,
        "should have exactly one epoch tag, got: {:?}",
        new_source
    );
}

#[test]
fn test_rewrite_injects_epoch_first_form() {
    let source = "(elle/epoch 0)\n(assert-true x \"test\")\n";
    let result = rewrite_file(source, "<test>").unwrap();
    assert!(result.is_some());
    let (new_source, _) = result.unwrap();
    let epoch_line = format!("(elle/epoch {})\n", CURRENT_EPOCH);
    assert!(
        new_source.starts_with(&epoch_line),
        "epoch tag should be the first form, got: {:?}",
        &new_source[..new_source.len().min(80)]
    );
    assert!(
        !new_source.contains("(elle/epoch 0)"),
        "old epoch tag should be removed"
    );
    // Verify no double epoch tags
    let epoch_count = new_source.matches("elle/epoch").count();
    assert_eq!(
        epoch_count, 1,
        "should have exactly one epoch tag, got: {:?}",
        new_source
    );
}

#[test]
fn test_rewrite_no_epoch_tag_injects_one() {
    // File without an epoch tag gets one added (current epoch).
    let source = "(println \"hello\")\n";
    let result = rewrite_file(source, "<test>").unwrap();
    assert!(result.is_some(), "epoch tag should be injected");
    let (new_source, _) = result.unwrap();
    let epoch_line = format!("(elle/epoch {})\n", CURRENT_EPOCH);
    assert!(
        new_source.starts_with(&epoch_line),
        "epoch tag should be first form, got: {:?}",
        &new_source[..new_source.len().min(80)]
    );
}

// --- the token-level pass (docs/impl/lexicon.md) ---

/// The source `edits` produce when applied.
fn applied(source: &str, mut edits: Vec<Edit>) -> String {
    apply_edits(source, &mut edits).unwrap()
}

/// `source` as a file read under `lexicon`.
fn read_under(source: &str, lexicon: Lexicon) -> SourceText<'_> {
    SourceText::new(source, "t.lisp", lexicon)
}

#[test]
fn a_comment_is_respelled_into_the_target_lexicon() {
    // Read under a lexicon that comments with `;`, written back out under
    // one that comments with `#`. The comment's own text is untouched.
    let source = "; note\n(def x 1)\n";
    let edits = collect_lexical_edits(
        read_under(source, Lexicon::divergent()),
        Lexicon::current(),
        &[],
    )
    .unwrap();
    assert_eq!(applied(source, edits), "# note\n(def x 1)\n");
}

#[test]
fn a_shebang_line_is_never_respelled() {
    // `#!/usr/bin/env elle` lexes as a comment under every lexicon that
    // comments with `#`, so the pass sees it as ordinary Elle trivia. It is
    // the operating system's line: respelling its first byte produces a file
    // the kernel will not run.
    let source = "#!/usr/bin/env elle\n# note\n(def x 1)\n";
    let edits = collect_lexical_edits(
        read_under(source, Lexicon::current()),
        Lexicon::divergent(),
        &[],
    )
    .unwrap();
    assert_eq!(
        applied(source, edits),
        "#!/usr/bin/env elle\n; note\n(def x 1)\n"
    );
}

#[test]
fn a_token_with_no_spelling_in_the_target_names_its_position() {
    let source = "(def x 1)\n(f ;xs)\n";
    let err = collect_lexical_edits(
        read_under(source, Lexicon::current()),
        Lexicon::divergent(),
        &[],
    )
    .unwrap_err();
    assert!(err.contains("t.lisp:2:4"), "{err}");
}

#[test]
fn a_file_under_one_lexicon_needs_no_lexical_edits() {
    // Read and written under the same rules, no token has another spelling:
    // the pass must add nothing to the rewrite.
    let source = "# note\n(def x \"\\n\")\n";
    let edits = collect_lexical_edits(
        read_under(source, Lexicon::current()),
        Lexicon::current(),
        &[],
    )
    .unwrap();
    assert!(edits.is_empty());
}

#[test]
fn a_file_at_epoch_12_has_each_escape_whose_meaning_moved_respelled() {
    // Epoch 12 read `\x41` as `x41` and `\0` as `0`. The rewritten file must
    // hold the same strings under the current rules, and keep the `\n` it
    // already spelled the same way under both.
    let source = "(elle/epoch 12)\n(def s \"\\x41\\n\\0\")\n";
    let (new_source, _) = rewrite_file(source, "<test>").unwrap().unwrap();
    assert_eq!(
        new_source,
        format!("(elle/epoch {CURRENT_EPOCH})\n(def s \"x41\\n0\")\n")
    );
}

#[test]
fn a_file_at_the_current_epoch_keeps_its_escapes() {
    let source = format!("(elle/epoch {CURRENT_EPOCH})\n(def s \"\\x41\")\n");
    assert!(rewrite_file(&source, "<test>").unwrap().is_none());
}

// --- the shorthand desugar pass (docs/impl/lexicon.md) ---

/// `source` read under the current lexicon, with `shorthands` spelled out.
///
/// No epoch declares a `Desugar` rule yet, so these tests name the shorthand
/// directly rather than reach the pass through `MIGRATIONS`.
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
    // and `(spliceargs)` for the gapless spelling; one of the two has to be
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
    // target lexicon has no bytes for it, which is right when nothing else
    // handles it and wrong once a `Desugar` rule does — the refusal would
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
