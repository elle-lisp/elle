// audited: 2026-11-18
// A file's name selects no syntax: the elle binary runs a file of any name as Elle, and its help names .lisp and .md alone.
//
// docs/impl/reader.md

use std::process::Command;

fn elle() -> Command {
    Command::new(env!("CARGO_BIN_EXE_elle"))
}

#[test]
fn help_names_s_expressions_and_markdown_as_the_only_syntaxes() {
    // Counter-factual: the help listed `.py`, `.js` and `.lua` as syntaxes
    // the binary reads, after nothing read them any more.
    let output = elle().arg("--help").output().expect("Failed to run elle");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(".lisp"), "got: {stdout}");
    assert!(stdout.contains(".md"), "got: {stdout}");
    for other in [".py", ".js", ".lua"] {
        assert!(!stdout.contains(other), "--help names {other}: {stdout}");
    }
}

#[test]
fn a_file_named_for_another_language_runs_as_elle() {
    // Counter-factual: a `.py`, `.js` or `.lua` file went to a reader for
    // that language, which refused this program or read it as something else.
    let dir = crate::common::ScratchDir::new("source-names");
    for name in ["prog.py", "prog.js", "prog.lua"] {
        let path = dir.join(name);
        std::fs::write(&path, "(println (+ 1 2))\n").expect("write scratch file");
        let output = elle().arg(&path).output().expect("Failed to run elle");
        assert_eq!(
            output.status.code().unwrap_or(-1),
            0,
            "{name}: stderr: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "3\n", "{name}");
    }
}
