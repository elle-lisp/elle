// audited: 2026-09-09
// What the runtime writes for a person to read. The `debug/print` and `trace`
// prints, and the unresolved-name canary over every surface a program reaches.
// docs/impl/symbol.md
//
// A render that threads no symbol table spells a symbol `#<symbol:hash>` and a
// keyword `#<keyword:hash>`. Those forms are deliberately unreadable, which
// makes them a canary: one in user-facing output means a render dropped the
// memo. The tests drive the binary, because the subject is what lands on the
// terminal rather than what a primitive returns.

use std::io::Write;
use std::process::{Command, Stdio};

/// Run `input` through the binary's read-stdin path and return its
/// `(stdout, stderr, exited-zero)`.
fn run(input: &str) -> (String, String, bool) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_elle"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn elle");
    child
        .stdin
        .as_mut()
        .expect("stdin is piped")
        .write_all(input.as_bytes())
        .expect("write the program");
    let out = child.wait_with_output().expect("wait for elle");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status.success(),
    )
}

/// The stderr of a program that must run cleanly.
///
/// Panics on a nonzero exit. A failed run never reaches the print under test,
/// and an empty stderr satisfies none of the assertions below but explains
/// nothing about why.
fn stderr_of(input: &str) -> String {
    let (_, stderr, ok) = run(input);
    assert!(ok, "{input:?} must run cleanly, stderr:\n{stderr}");
    stderr
}

#[test]
fn debug_print_names_a_keyword_it_prints() {
    // The counter-factual: probe with a keyword the static vocabulary already
    // spells (`:timeout`, `:ok`, any error kind) and `resolve_keyword_name`
    // answers from `VOCABULARY` whether or not the print threads the memo. Only
    // a spelling this instance learned from source separates the two.
    let err = stderr_of("(debug/print :probe-debug-keyword)\n");
    assert!(
        err.contains(":probe-debug-keyword"),
        "debug/print must name the keyword it prints, got:\n{err}"
    );
}

#[test]
fn debug_print_names_a_symbol_it_prints() {
    // A symbol has no vocabulary behind it, so an unthreaded print hashes every
    // one of them.
    let err = stderr_of("(debug/print (quote probe-debug-symbol))\n");
    assert!(
        err.contains("probe-debug-symbol"),
        "debug/print must name the symbol it prints, got:\n{err}"
    );
}

#[test]
fn trace_names_the_keyword_it_traces() {
    // The trap: `trace` resolves its LABEL through the memo already, so a test
    // that checks the label alone passes while the value beside it on the same
    // line reads `#<keyword:hash>`.
    let err = stderr_of("(trace \"probe-label\" :probe-trace-keyword)\n");
    assert!(
        err.contains(":probe-trace-keyword"),
        "trace must name the keyword it traces, got:\n{err}"
    );
}

#[test]
fn trace_names_a_keyword_nested_in_the_traced_value() {
    // A traced value is usually a container. Threading the memo into the outer
    // render but not through its recursion leaves every nested name hashed.
    let err = stderr_of("(trace \"probe-label\" [1 :probe-nested-keyword])\n");
    assert!(
        err.contains(":probe-nested-keyword"),
        "trace must name a keyword inside the value it traces, got:\n{err}"
    );
}

/// Every surface a program's own names reach, in one program: the value the
/// REPL echoes, `print`, `string`, the two diagnostic prints, the message of a
/// type error, and the report of an error that reaches the root.
///
/// The last form raises, so the program exits nonzero. That is deliberate, and
/// the test below reads both streams rather than the status.
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
    // The net for the whole class. The tests above say which name each print
    // must spell; this one says no surface may carry the unreadable form at
    // all, so a render added later that drops the memo fails here even though
    // no test names it.
    //
    // The counter-factual: probe with names the vocabulary spells (`:ok`,
    // `:timeout`, `:type-error`) and every one of them resolves with no memo
    // threaded anywhere.
    let (stdout, stderr, _) = run(EVERY_SURFACE);
    for (stream, text) in [("stdout", &stdout), ("stderr", &stderr)] {
        for form in ["#<keyword:", "#<symbol:"] {
            assert!(
                !text.contains(form),
                "{stream} carries the unresolved form {form}, so a render dropped \
                 the symbol memo:\n{text}"
            );
        }
    }
    // Guard the guard: an empty stream carries no unresolved form either.
    assert!(
        stdout.contains("canary-printed") && stderr.contains("canary-raised"),
        "the probe must reach every surface it names:\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}
