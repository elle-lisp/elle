// The wall-clock budget the corpus per-file passes give one file.
//
// `RUN_PER_FILE` runs every corpus file as its own process under `timeout`, and
// a file that outlives its budget is killed: exit 124, no output, no assertion
// message. That budget is a single number for the whole corpus, and some files
// spend most of it on work they need — the h2 families drive hundreds of
// requests over one session, and one file reads 20000 lines to drive a function
// hot enough for the JIT to compile it. On an idle box each finishes with
// seconds to spare; on a loaded CI runner it does not, and the gate reports a
// kill rather than a defect.
//
// So those are named in the Makefile and given a wider budget. The names are
// shell text, and every half of that fails quietly: a renamed file stops
// matching and silently drops back to the narrow budget, a new pass that spells
// the narrow budget directly gives it to every file, and a file that grows its
// own deadline is connected to none of it. Nothing compiles any of them. These
// tests are the standing check, and they cost a read of the Makefile and a few
// `sh` invocations.
//
// The trap, and why the selector is executed here rather than pattern-matched:
// the shell that parses it is the platform's `/bin/sh`, and they do not agree.
// bash 3.2, which is what macOS supplies, ends a `$(…)` at the first `)` inside
// it, so a construct that carries one — a `case` pattern — parses on the
// development box and dies file by file on the macOS runner.

use crate::common::make_var;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn makefile() -> String {
    fs::read_to_string(repo_root().join("Makefile")).expect("read the Makefile")
}

/// One Makefile variable, as `make` expands it.
///
/// Asking `make` rather than parsing the assignment is the whole point: these
/// tests measure what the corpus pass will actually run, and a parser that
/// reimplements variable references, line continuations and `$(shell …)` is a
/// second `make` that can disagree with the first.
fn expand(name: &str) -> String {
    make_var(name, &[]).unwrap_or_else(|| panic!("`make print-{name}` did not run"))
}

/// The patterns the Makefile gives the wider budget.
///
/// `WIDE_FILES` is a `grep` pattern list, `-e one -e two` — the shape the
/// per-pass skip lists beside it already use. A pattern is a substring of a
/// path, not a file name: a whole family of corpus files shares one deadline
/// and one prefix, so the list names the prefix rather than every member.
fn wide_patterns() -> Vec<String> {
    let patterns = expand("WIDE_FILES");
    let names: Vec<String> = patterns
        .split_whitespace()
        .filter(|word| *word != "-e")
        .map(str::to_string)
        .collect();
    assert!(
        !names.is_empty(),
        "WIDE_FILES does not read as a `grep` pattern list: {patterns}"
    );
    names
}

/// Every corpus file the per-file passes run, as a repo-relative path.
fn corpus_files() -> Vec<String> {
    let mut paths: Vec<String> = fs::read_dir(repo_root().join("tests/elle"))
        .expect("read tests/elle")
        .map(|entry| entry.expect("a corpus directory entry").file_name())
        .filter_map(|name| name.to_str().map(str::to_string))
        .filter(|name| name.ends_with(".lisp"))
        .map(|name| format!("tests/elle/{name}"))
        .collect();
    paths.sort();
    assert!(paths.len() > 100, "the corpus did not read: {paths:?}");
    paths
}

/// The deadline a corpus file gives itself, if it declares one.
///
/// A file that has to detect a stall carries `(def deadline N)` and reports
/// through it — which request stalled, and how long it waited. That number is
/// in seconds, and it is the only thing that knows what the file considers
/// hung.
fn declared_deadline(path: &str) -> Option<u64> {
    let source = fs::read_to_string(repo_root().join(path)).expect("read a corpus file");
    let (_, rest) = source.split_once("(def deadline ")?;
    let digits = rest.split(')').next()?.trim();
    Some(
        digits
            .parse()
            .unwrap_or_else(|_| panic!("{path} declares a deadline this cannot read: {digits}")),
    )
}

/// The Makefile's budget selector with `{}` replaced by `path`, ready for `sh`.
fn expanded_budget_selector(path: &str) -> String {
    let selector = expand("FILE_TIMEOUT");
    // `{}` is where `parallel` puts the path. A selector without one is still
    // valid shell and still prints a budget — the same budget for every file —
    // so nothing downstream would notice, and every test here would measure a
    // constant while believing it measured a choice.
    assert!(
        selector.contains("{}"),
        "the budget selector never names the file `parallel` substitutes: {selector}"
    );
    selector.replace("{}", path)
}

/// A `timeout` argument as a number: `120s` is 120.
fn seconds(budget: &str) -> u64 {
    budget
        .trim()
        .trim_end_matches('s')
        .parse()
        .unwrap_or_else(|_| panic!("a budget is a whole number of seconds: {budget}"))
}

/// The budget one corpus file gets, read the way the pass reads it: `parallel`
/// substitutes the path into the selector and evaluates it in `sh`, which is
/// the shell a make recipe runs under.
fn budget_for(path: &str) -> String {
    let script = format!("printf '%s' {}", expanded_budget_selector(path));
    let out = Command::new("/bin/sh")
        .arg("-c")
        .arg(&script)
        .output()
        .expect("run the budget selector");
    assert!(
        out.status.success(),
        "the budget selector is not valid `sh`: {script}"
    );
    String::from_utf8(out.stdout).expect("the selector prints a budget")
}

// A `WIDE_FILES` pattern that matches nothing is not an error anywhere: `grep`
// simply never fires it, and the file it was written for — under whatever name
// it now has — goes back to the narrow budget and starts dying on exit 124
// under load. The counter-factual: rename a heavy corpus file without touching
// the Makefile, and every gate still passes until a runner is slow enough.
#[test]
fn every_pattern_named_for_the_wider_budget_matches_a_corpus_file() {
    let corpus = corpus_files();
    for pattern in wide_patterns() {
        assert!(
            corpus.iter().any(|path| path.contains(&pattern)),
            "the Makefile gives `{pattern}` the wider per-file budget, but no \
             corpus file matches it. `grep` never fires the pattern, so \
             whatever those files are called now run under TIMEOUT."
        );
    }
}

// The selector is shell text in a make variable: nothing compiles it, and a
// pattern that matches nothing still runs — every file just takes the fallback.
// This drives the real thing under a real shell, once per outcome. The
// counter-factual: misspell one name so it can never match a path, and the two
// heavy files quietly return to the narrow budget.
#[test]
fn the_selector_widens_the_named_files_and_nothing_else() {
    let wide = expand("WIDE_TIMEOUT");
    let narrow = expand("TIMEOUT");
    assert!(
        seconds(&wide) > seconds(&narrow),
        "WIDE_TIMEOUT is {wide}, which is not wider than TIMEOUT at {narrow}"
    );

    for pattern in wide_patterns() {
        for path in corpus_files().iter().filter(|p| p.contains(&pattern)) {
            assert_eq!(
                budget_for(path),
                wide,
                "{path} matches the WIDE_FILES pattern `{pattern}` but the \
                 selector gives it {narrow}"
            );
        }
    }

    // Ordinary corpus files keep the narrow budget: it is what makes a hang
    // fail fast, and widening it for everything would trade that away. Both
    // controls have to exist, or this arm is measured against a path the pass
    // would never run.
    for path in ["tests/elle/arithmetic.lisp", "tests/elle/strings.lisp"] {
        assert!(repo_root().join(path).exists(), "{path} is gone");
        assert_eq!(
            budget_for(path),
            narrow,
            "{path} is not named in WIDE_FILES but the selector widened it"
        );
    }
}

// The trap, guarded where it can be caught. bash 3.2 — macOS's `/bin/sh` —
// ends a `$(…)` at the first `)` inside it, so a selector carrying one is cut
// in half before the shell ever reads what it does. Every later shell parses
// the same text correctly, which is what makes this expensive: it passes on the
// development box, passes on Linux CI, and takes the whole corpus down file by
// file on the macOS runner with a syntax error naming none of it. The
// counter-factual is the obvious spelling — `$(case {} in */a.lisp) echo …;;
// *) echo …;; esac)` — which reads better, works everywhere it is tried, and
// cannot ship.
#[test]
fn the_selector_carries_no_parenthesis_a_shell_could_end_it_on() {
    let selector = expanded_budget_selector("tests/elle/arithmetic.lisp");
    let inner = selector
        .trim()
        .strip_prefix("$(")
        .and_then(|rest| rest.strip_suffix(')'))
        .unwrap_or_else(|| panic!("the budget selector is not one substitution: {selector}"));
    assert!(
        !inner.contains('(') && !inner.contains(')'),
        "the budget selector carries a parenthesis: {selector}\n\
         macOS's /bin/sh ends the substitution at the first one, so the corpus \
         dies on a syntax error there while every other platform passes."
    );
}

// A pass that writes `timeout $(TIMEOUT)` into its own command line hands that
// budget to every file it runs, including the two that cannot fit it — and the
// failure is exit 124 with no output, which reads as a flaky runner rather than
// as a budget that was never wide enough. Every pass over the corpus takes its
// budget from the selector, which is the only thing that knows about them.
#[test]
fn no_corpus_pass_spells_the_narrow_budget_directly() {
    // Recipe lines only: a recipe is what runs, and the comment above
    // FILE_TIMEOUT quotes both spellings to contrast them.
    let offender = makefile()
        .lines()
        .find(|line| line.starts_with('\t') && line.contains("timeout $(TIMEOUT)"))
        .map(str::to_string);
    assert!(
        offender.is_none(),
        "a Makefile recipe runs `timeout $$(TIMEOUT)` directly:\n  {}\n\
         Use `timeout $$(FILE_TIMEOUT)`, which gives the files named in \
         WIDE_FILES the budget they need and every other file TIMEOUT.",
        offender.unwrap_or_default().trim()
    );
}

// The outer budget is a backstop, not the deadline. A file that carries its own
// `deadline` reports on it — which request stalled, how long it waited — and
// that message is the reason to run the file at all. If the outer `timeout`
// fires first the message is never printed: the process dies on a signal and
// the gate reports exit 124.
//
// This asks every corpus file, not only the ones already named in WIDE_FILES.
// Naming is the half that rots: a file grows a deadline, or copies a sibling
// that has one, and nothing connects that number to the budget the pass will
// actually hand it. The counter-factual is what this found — thirteen h2 files
// declaring deadlines of 60 s and 120 s while running under a 30 s budget, so
// none of them could ever print the diagnostic they exist to print.
#[test]
fn no_corpus_file_outlives_the_budget_before_its_own_deadline_fires() {
    let mut checked = 0;
    for path in corpus_files() {
        let Some(deadline) = declared_deadline(&path) else {
            continue;
        };
        let budget = budget_for(&path);
        assert!(
            seconds(&budget) > deadline,
            "{path} gives itself {deadline} s to report a stall, and the pass \
             kills it at {budget}. The kill lands first, so the file's own \
             diagnostic can never print — the gate reports exit 124 with no \
             output, which reads as a flaky runner. Either name the file in \
             WIDE_FILES or lower the deadline below TIMEOUT."
        );
        checked += 1;
    }
    assert!(
        checked > 0,
        "no corpus file declares a deadline. Either the declaration changed \
         shape or the argument no longer applies — teach this test the new \
         shape rather than letting it pass by matching nothing."
    );
}
