// audited: 2026-11-18
// `elle rewrite --list-rules` prints every migration rule under the epoch that adds it and that epoch's summary.
// docs/epochs.md

use std::process::Command;

/// What `elle rewrite --list-rules` prints, after it exits zero.
fn listing() -> String {
    let output = Command::new(env!("CARGO_BIN_EXE_elle"))
        .args(["rewrite", "--list-rules"])
        .output()
        .expect("spawn elle");
    assert!(
        output.status.success(),
        "stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).into_owned()
}

/// Where `needle` starts in `haystack`, or a failure that shows the listing.
fn position(haystack: &str, needle: &str) -> usize {
    haystack
        .find(needle)
        .unwrap_or_else(|| panic!("--list-rules must print {needle:?}, got:\n{haystack}"))
}

#[test]
fn every_epoch_heads_its_rules_in_order() {
    let out = listing();
    let current: u64 = out
        .split("current epoch: ")
        .nth(1)
        .and_then(|rest| rest.split(')').next())
        .and_then(|n| n.parse().ok())
        .unwrap_or_else(|| panic!("the first line names the current epoch:\n{out}"));
    let mut last = 0;
    for epoch in 1..=current {
        let at = position(&out, &format!("\nepoch {epoch}: "));
        assert!(
            at > last,
            "epoch {epoch} comes after the one before it:\n{out}"
        );
        last = at;
    }
}

#[test]
fn a_rule_stands_under_the_epoch_that_adds_it() {
    // A flat list hid which epoch a rule belongs to, so the reader of a file
    // that declares epoch 15 could not tell which rules reach it.
    let out = listing();
    let rule = position(&out, "replace: import-file (arity 1) → (import $1)");
    assert!(
        position(&out, "\nepoch 16: import-file goes") < rule,
        "{out}"
    );
    assert!(rule < position(&out, "\nepoch 17: "), "{out}");

    let escapes = position(&out, "lexical: string-escapes");
    assert!(position(&out, "\nepoch 13: ") < escapes, "{out}");
    assert!(escapes < position(&out, "\nepoch 14: "), "{out}");
}

#[test]
fn a_rename_is_listed_as_its_epoch_wrote_it() {
    // Epoch 3 renames display to print, and epoch 2 renamed print to println.
    // Each epoch's own rule is what a file of that epoch meets.
    let out = listing();
    let rule = position(&out, "rename: display → print\n");
    assert!(position(&out, "\nepoch 3: ") < rule, "{out}");
    assert!(rule < position(&out, "\nepoch 4: "), "{out}");
}

#[test]
fn the_calls_whose_durations_moved_to_seconds_are_named() {
    // Epoch 14 rewrites a millisecond bound as `:timeout` in seconds. A rule
    // the listing leaves out is a rewrite nobody reading it knows about.
    let out = listing();
    for call in [
        "sys/join",
        "chan/select",
        "io/wait",
        "ev/step",
        "udp/recv-from",
    ] {
        position(&out, call);
    }
}

#[test]
fn the_letrec_rule_stands_under_epoch_18() {
    let out = listing();
    let rule = position(&out, "defs-into-letrec: ");
    assert!(position(&out, "\nepoch 18: ") < rule, "{out}");
}
