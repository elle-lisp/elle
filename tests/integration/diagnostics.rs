// What the runtime writes for a person to read: the diagnostic primitives
// `debug/print` and `trace`, and the unresolved-name canary over every surface
// a program's output reaches.
//
// A render that carries no symbol table spells a symbol `#<symbol:hash>` and a
// keyword `#<keyword:hash>` (docs/impl/symbol.md § "Reading a name, and not
// reading one"). Those forms are deliberately unreadable, which makes them a
// canary: one in user-facing output means a formatter did not thread the memo.
// The tests drive the binary because the subject is what lands on the terminal,
// not what a primitive returns.

use std::io::Write;
use std::process::{Command, Stdio};

/// Run `input` through the binary's REPL-on-stdin path and return its
/// `(stdout, stderr, exited-zero)`.
fn run(input: &str) -> (String, String, bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_elle"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn elle");
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.success(),
    )
}

/// The stderr of a program that must run cleanly.
///
/// Panics on a nonzero exit: a failure means the run never reached the print
/// under test, and an empty stderr would satisfy the assertions below.
fn stderr_of(input: &str) -> String {
    let (_, stderr, ok) = run(input);
    assert!(
        ok,
        "{:?} was expected to run cleanly:\nstderr: {}",
        input, stderr
    );
    stderr
}

#[test]
fn debug_print_names_a_keyword_it_prints() {
    // The counter-factual: probe with a keyword the static vocabulary already
    // spells (`:timeout`, `:ok`, any error kind) and the fallback in
    // `keyword::static_keyword_name` answers whether or not the print threads
    // the memo. Only a spelling read from this source discriminates them.
    let err = stderr_of("(debug/print :probe-debug-keyword)\n");
    assert!(
        err.contains(":probe-debug-keyword"),
        "debug/print must name the keyword it prints, got: {}",
        err
    );
}

#[test]
fn debug_print_names_a_symbol_it_prints() {
    let err = stderr_of("(debug/print (quote probe-debug-symbol))\n");
    assert!(
        err.contains("probe-debug-symbol"),
        "debug/print must name the symbol it prints, got: {}",
        err
    );
}

#[test]
fn trace_names_the_keyword_it_traces() {
    // The trap: `trace` already resolves its LABEL through the memo, so a test
    // that only checks the label passes over a value rendered `#<keyword:hash>`
    // on the same line.
    let err = stderr_of("(trace \"probe-label\" :probe-trace-keyword)\n");
    assert!(
        err.contains(":probe-trace-keyword"),
        "trace must name the keyword it traces, got: {}",
        err
    );
}

#[test]
fn trace_names_a_keyword_nested_in_the_traced_value() {
    let err = stderr_of("(trace \"probe-label\" [1 :probe-nested-keyword])\n");
    assert!(
        err.contains(":probe-nested-keyword"),
        "trace must name a keyword inside the value it traces, got: {}",
        err
    );
}

/// Every surface a program's own names reach, in one program: the value the
/// REPL echoes, `print`, `string`, the diagnostic prints, the message of a
/// type error, and the report of an error that reaches the root.
///
/// The last form raises, so the program exits nonzero — that is the point, and
/// `unresolved_names_reach_no_user_facing_surface` reads both streams.
const EVERY_SURFACE: &str = concat!(
    ":canary-echoed\n",
    "(print [:canary-printed (quote canary-printed-sym)])\n",
    "(print (string {:canary-stringified 1}))\n",
    "(debug/print :canary-debugged)\n",
    "(trace \"canary\" :canary-traced)\n",
    "(print (protect ((quote canary-uncallable) 1)))\n",
    "(error {:code :canary-raised :at (quote canary-raised-sym)})\n",
);

#[test]
fn unresolved_names_reach_no_user_facing_surface() {
    // The canary net for the whole class: a formatter anywhere in the pipeline
    // that renders a value without the memo puts `#<keyword:hash>` in front of
    // the author. The per-surface tests above say which name must appear; this
    // one says no surface may print the unreadable form at all, so a NEW
    // formatter that forgets the memo fails here even though no test names it.
    //
    // The counter-factual: probe with names the static vocabulary spells
    // (`:ok`, `:timeout`, `:type-error`) and `keyword::static_keyword_name`
    // answers for every one of them with no memo threaded anywhere.
    let (stdout, stderr, _) = run(EVERY_SURFACE);
    for (stream, text) in [("stdout", &stdout), ("stderr", &stderr)] {
        for form in ["#<keyword:", "#<symbol:"] {
            assert!(
                !text.contains(form),
                "{} printed the unresolved form {}, so some formatter dropped the \
                 symbol memo:\n{}",
                stream,
                form,
                text,
            );
        }
    }
    // Guard the guard: an empty stream trivially contains no unresolved form.
    assert!(
        stdout.contains("canary-printed") && stderr.contains("canary-raised"),
        "the probe program must reach every surface it names:\nstdout: {}\nstderr: {}",
        stdout,
        stderr,
    );
}
