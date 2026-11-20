// audited: 2026-11-18
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

// --- a file at an older epoch ---

/// `body` written at `epoch`, rewritten, with the new epoch tag taken off.
fn rewritten_from(epoch: u64, body: &str) -> String {
    let source = format!("(elle/epoch {epoch})\n{body}");
    let (new_source, _) = rewrite_file(&source, "<test>").unwrap().unwrap();
    new_source
        .strip_prefix(&format!("(elle/epoch {CURRENT_EPOCH})\n"))
        .unwrap_or_else(|| panic!("no current epoch tag in {new_source:?}"))
        .to_string()
}

/// `source` read, migrated from epoch `from` by the tree rules, and printed
/// back one form to a line, or the error the migration raises.
fn migrated_from(from: u64, source: &str) -> Result<String, String> {
    let arena = crate::syntax::thread_arena();
    let mut forms = crate::reader::read_syntax_all(arena, source, "<test>").unwrap();
    crate::epoch::transform::migrate(&arena, &mut forms, from, CURRENT_EPOCH)?;
    Ok(forms
        .iter()
        .map(|f| f.to_string())
        .collect::<Vec<_>>()
        .join("\n"))
}

mod defs;
mod desugar;
mod forward;
mod imports;
mod millis;
mod signals;
