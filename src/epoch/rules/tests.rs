// audited: 2026-09-23
//! Tests for the epoch rule tables, the collectors that read them, and the
//! lexicon's respelling of a token.
//!
//! docs/epochs.md
//! docs/impl/lexicon.md

use super::*;

#[test]
fn test_empty_range() {
    let renames = collapsed_renames(0, 0);
    assert!(renames.is_empty());
}

#[test]
fn test_renames_through_current() {
    let renames = collapsed_renames(0, CURRENT_EPOCH);
    // epoch 2: print→println, newline→println
    // epoch 3: display→print
    // epoch 4: stream/{read,read-line,read-all,write,flush} → port/...
    assert_eq!(renames.get("print"), Some(&"println"));
    assert_eq!(renames.get("newline"), Some(&"println"));
    assert_eq!(renames.get("display"), Some(&"print"));
    assert_eq!(renames.get("stream/read-line"), Some(&"port/read-line"));
    assert_eq!(renames.get("stream/read"), Some(&"port/read"));
    assert_eq!(renames.get("stream/read-all"), Some(&"port/read-all"));
    assert_eq!(renames.get("stream/write"), Some(&"port/write"));
    assert_eq!(renames.get("stream/flush"), Some(&"port/flush"));
    // epoch 5: string-contains?→has?, string/contains?→has?
    assert_eq!(renames.get("string-contains?"), Some(&"has?"));
    assert_eq!(renames.get("string/contains?"), Some(&"has?"));
    // epoch 10: cons→pair, car→first, cdr→rest
    assert_eq!(renames.get("cons"), Some(&"pair"));
    assert_eq!(renames.get("car"), Some(&"first"));
    assert_eq!(renames.get("cdr"), Some(&"rest"));
    // epoch 11: sys/spawn→sys/spawn-vm, os/spawn→os/spawn-vm
    // (bare `spawn` is intentionally NOT renamed — it is also a local)
    assert_eq!(renames.get("spawn"), None);
    assert_eq!(renames.get("sys/spawn"), Some(&"sys/spawn-vm"));
    assert_eq!(renames.get("os/spawn"), Some(&"os/spawn-vm"));
    // epoch 12: coro/* → fiber/*, coroutine-* → fiber/*, etc.
    assert_eq!(renames.get("coro/resume"), Some(&"fiber/resume"));
    assert_eq!(renames.get("coro/status"), Some(&"fiber/status"));
    assert_eq!(renames.get("coro/done?"), Some(&"fiber/done?"));
    assert_eq!(renames.get("coro/value"), Some(&"fiber/value"));
    assert_eq!(renames.get("coroutine-resume"), Some(&"fiber/resume"));
    assert_eq!(renames.get("coroutine-status"), Some(&"fiber/status"));
    assert_eq!(renames.get("coroutine-done?"), Some(&"fiber/done?"));
    assert_eq!(renames.get("coroutine-value"), Some(&"fiber/value"));
    assert_eq!(renames.get("coroutine?"), Some(&"fiber?"));
    assert_eq!(renames.get("coro?"), Some(&"fiber?"));
    assert_eq!(renames.get("yield-from"), Some(&"yield*"));
    // 13 base renames (epochs 1–10) + 2 spawn (epoch 11) + 11 coro (epoch 12)
    assert_eq!(renames.len(), 26);
}

#[test]
fn test_replace_rules_empty_range() {
    let replaces = replace_rules_in_range(0, 0);
    assert!(replaces.is_empty());
}

#[test]
fn test_replace_rules_epoch_1() {
    let replaces = replace_rules_in_range(0, 1);
    assert_eq!(replaces.len(), 9);
    // First rule should be assert-true
    assert_eq!(replaces[0].0, "assert-true");
}

#[test]
fn test_removals_epoch_2() {
    let removals = removals_in_range(0, CURRENT_EPOCH);
    assert!(removals.contains_key("write"));
    // epoch 12: coro/>iterator, coroutine->iterator, coroutine-next
    assert!(removals.contains_key("coro/>iterator"));
    assert!(removals.contains_key("coroutine->iterator"));
    assert!(removals.contains_key("coroutine-next"));
    assert_eq!(removals.len(), 4);
}

#[test]
fn test_flatten_rules_epoch_7() {
    let flattens = flatten_rules_in_range(0, CURRENT_EPOCH);
    assert!(flattens.contains(&"let"));
    assert!(flattens.contains(&"letrec"));
    assert!(flattens.contains(&"let*"));
    assert!(flattens.contains(&"if-let"));
    assert!(flattens.contains(&"when-let"));
    assert!(flattens.contains(&"when-ok"));
    assert_eq!(flattens.len(), 6);
}

#[test]
fn test_rename_chaining() {
    // Simulate chained renames manually
    let mut table: HashMap<&str, &str> = HashMap::new();

    // Epoch 1: A → B
    table.insert("A", "B");

    // Epoch 2: B → C — should update A → C
    let original = table.iter().find(|(_, v)| **v == "B").map(|(k, _)| *k);
    if let Some(original) = original {
        table.insert(original, "C");
    } else {
        table.insert("B", "C");
    }

    assert_eq!(table.get("A"), Some(&"C"));
    assert!(!table.contains_key("B"));
}

// --- token-level respelling (docs/impl/lexicon.md) ---

use crate::reader::Lexer;

/// `target`'s spelling of the one token in `lexeme`, lexed under `from`.
///
/// The token comes from lexing the text, so the two cannot disagree the way
/// a hand-built pair could.
fn respelled(from: Lexicon, lexeme: &str, target: Lexicon) -> Result<Option<String>, String> {
    let token = Lexer::new(lexeme)
        .in_lexicon(from)
        .next_token()
        .unwrap()
        .unwrap();
    from.respell(&token, lexeme, &target)
}

/// The current lexicon's spelling of the one token in `lexeme`, lexed under
/// `from`.
fn into_current(from: Lexicon, lexeme: &str) -> Result<Option<String>, String> {
    respelled(from, lexeme, Lexicon::current())
}

#[test]
fn a_comment_keeps_its_spelling_when_the_introducer_is_unchanged() {
    assert_eq!(into_current(Lexicon::current(), "# c\n").unwrap(), None);
}

#[test]
fn a_comment_takes_the_introducer_of_the_target_lexicon() {
    // `divergent` comments with `;`; the current lexicon comments with `#`.
    // Only the introducer moves — the rest of the line is the author's text.
    assert_eq!(
        into_current(Lexicon::divergent(), "; c\n").unwrap(),
        Some("# c\n".to_string())
    );
}

#[test]
fn a_token_the_target_cannot_spell_is_refused_not_left_alone() {
    // `;` splices under one lexicon and starts a comment under the other,
    // so there is no text to put in its place. Answering "unchanged" would
    // leave the byte where it is and silently change what the file means.
    let err = respelled(Lexicon::current(), ";", Lexicon::divergent()).unwrap_err();
    assert!(err.contains(';'), "{err}");
}

#[test]
fn a_fused_unquote_splice_the_target_cannot_spell_is_refused() {
    let err = respelled(Lexicon::current(), ",;", Lexicon::no_semicolon()).unwrap_err();
    assert!(err.contains(",;"), "{err}");
}

/// The newest lexicon that drops the backslash of an unknown escape.
fn epoch_12() -> Lexicon {
    Lexicon::for_epoch(12)
}

/// The string the literal `lexeme` reads as under `lexicon`.
fn read_as(lexeme: &str, lexicon: Lexicon) -> String {
    match Lexer::new(lexeme).in_lexicon(lexicon).next_token() {
        Ok(Some(Token::String(s))) => s,
        other => panic!("{lexeme} lexed as {other:?}"),
    }
}

#[test]
fn a_string_whose_escapes_read_alike_keeps_its_spelling() {
    // The five shared escapes mean the same thing under every lexicon, so a
    // string written with only those needs no edit.
    assert_eq!(into_current(epoch_12(), r#""a\n\t\r\\\"b""#).unwrap(), None);
    assert_eq!(into_current(Lexicon::current(), r#""\x41""#).unwrap(), None);
}

#[test]
fn an_escape_whose_meaning_moved_becomes_the_text_epoch_12_read() {
    let cases = [
        (r#""\x41""#, r#""x41""#),
        (r#""\0""#, r#""0""#),
        (r#""\u{e9}""#, r#""u{e9}""#),
        (r#""\q""#, r#""q""#),
        (r#""\é""#, r#""é""#),
        (r#""\x80""#, r#""x80""#),
    ];
    for (old, new) in cases {
        assert_eq!(
            into_current(epoch_12(), old).unwrap(),
            Some(new.to_string()),
            "{old}"
        );
        // The new spelling must read, under the current rules, as the string
        // the old one read under epoch 12. Comparing text alone would pass a
        // respelling that reads as something else.
        assert_eq!(
            read_as(new, Lexicon::current()),
            read_as(old, epoch_12()),
            "{old}"
        );
    }
}

#[test]
fn a_respelled_string_keeps_its_shared_escapes_as_written() {
    // Only the escape whose meaning moved changes. The author's `\"` and `\n`
    // stay as written.
    assert_eq!(
        into_current(epoch_12(), r#""\"\x41\"\n""#).unwrap(),
        Some(r#""\"x41\"\n""#.to_string())
    );
}

#[test]
fn an_escaped_line_break_becomes_the_newline_escape() {
    // Epoch 12 reads a backslash before a line break as the line break, and
    // the current lexicon refuses that escape. `\n` spells the same string.
    let old = "\"a\\\nb\"";
    assert_eq!(
        into_current(epoch_12(), old).unwrap(),
        Some(r#""a\nb""#.to_string())
    );
    assert_eq!(
        read_as(r#""a\nb""#, Lexicon::current()),
        read_as(old, epoch_12())
    );
}

#[test]
fn every_desugar_rule_names_a_token_the_reader_wraps_a_form_in() {
    // The rule carries the token alone and reads the form from it, so a rule
    // naming a token that stands for no form has nothing to rewrite to. The
    // pass would refuse it at run time; this refuses it at test time, before
    // an epoch ships one.
    for shorthand in desugar_rules_in_range(0, CURRENT_EPOCH) {
        assert!(
            shorthand.shorthand_form().is_some(),
            "{shorthand:?} is not a reader shorthand"
        );
    }
}

#[test]
fn an_epoch_declares_a_lexical_change_exactly_when_its_lexicon_moves() {
    // The descriptor is what `--list-rules` and the epoch history read. A
    // lexicon that moves without one is a breaking change nothing tells the
    // author about; a descriptor without a moved lexicon describes a change
    // that did not happen.
    for epoch in 1..=CURRENT_EPOCH {
        let moved = Lexicon::for_epoch(epoch) != Lexicon::for_epoch(epoch - 1);
        let declared = lexical_changes_in_range(epoch - 1, epoch).next().is_some();
        assert_eq!(moved, declared, "epoch {epoch}");
    }
}
