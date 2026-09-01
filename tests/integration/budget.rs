// The wall-clock budget the corpus per-file passes give one file.
//
// `RUN_PER_FILE` runs every corpus file as its own process under `timeout`, and
// a file that outlives its budget is killed: exit 124, no output, no assertion
// message. That budget is a single number for the whole corpus, and two files
// spend most of it on work they need — one drives 500 requests over one h2
// session, the other reads 20000 lines to drive a function hot enough for the
// JIT to compile it. On an idle box each finishes with seconds to spare; on a
// loaded CI runner it does not, and the gate reports a kill rather than a
// defect.
//
// So those two are named in the Makefile and given a wider budget. The names
// are shell text, and both halves fail quietly: a renamed file stops matching
// and silently drops back to the narrow budget, and a new pass that spells the
// narrow budget directly gives it to every file. Nothing compiles either one.
// These tests are the standing check, and they cost a read of the Makefile and
// a few `sh` invocations.
//
// The trap, and why the selector is executed here rather than pattern-matched:
// the shell that parses it is the platform's `/bin/sh`, and they do not agree.
// bash 3.2, which is what macOS supplies, ends a `$(…)` at the first `)` inside
// it, so a construct that carries one — a `case` pattern — parses on the
// development box and dies file by file on the macOS runner.

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn makefile() -> String {
    fs::read_to_string(repo_root().join("Makefile")).expect("read the Makefile")
}

/// Every simple variable assignment in the Makefile, by name.
///
/// A definition opens at column zero and its operator is one of `:=`, `?=` or
/// `=`. Rules (`target: prerequisite`) and recipe lines are not assignments and
/// do not land here.
fn assignments(text: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        if line.starts_with([' ', '\t', '#']) {
            continue;
        }
        let Some((head, value)) = line.split_once('=') else {
            continue;
        };
        let name = head.trim_end_matches([':', '?', '+']).trim();
        if name.is_empty() || !name.chars().all(|c| c.is_ascii_uppercase() || c == '_') {
            continue;
        }
        out.insert(name.to_string(), value.trim().to_string());
    }
    out
}

/// One Makefile variable, expanded.
///
/// Only `$(NAME)` references to other variables are resolved; a `$(...)` whose
/// body is not a bare variable name — the shell substitution below is one — is
/// left alone. `expanded_budget_selector` asserts that nothing unresolved
/// remains, so a reference this cannot follow fails the test rather than
/// reaching `sh` as literal text.
fn expand(name: &str, vars: &BTreeMap<String, String>) -> String {
    let mut text = vars
        .get(name)
        .unwrap_or_else(|| panic!("the Makefile defines {name}"))
        .clone();
    for _ in 0..8 {
        let mut next = String::new();
        let mut rest = text.as_str();
        while let Some(at) = rest.find("$(") {
            let (before, from) = rest.split_at(at);
            next.push_str(before);
            let body = &from[2..];
            match body.split_once(')') {
                Some((inner, after)) if vars.contains_key(inner) => {
                    next.push_str(&vars[inner]);
                    rest = after;
                }
                _ => {
                    next.push_str("$(");
                    rest = body;
                }
            }
        }
        next.push_str(rest);
        if next == text {
            return text;
        }
        text = next;
    }
    panic!("{name} expands without settling; a variable references itself");
}

/// The names the Makefile gives the wider budget, as corpus file names.
///
/// `WIDE_FILES` is a `grep` pattern list, `-e one.lisp -e two.lisp` — the shape
/// the per-pass skip lists beside it already use.
fn wide_file_names() -> Vec<String> {
    let text = makefile();
    let patterns = expand("WIDE_FILES", &assignments(&text));
    let names: Vec<String> = patterns
        .split_whitespace()
        .filter(|word| *word != "-e")
        .map(str::to_string)
        .collect();
    assert!(
        !names.is_empty() && names.iter().all(|n| n.ends_with(".lisp")),
        "WIDE_FILES does not read as a `grep` pattern list over corpus files: {patterns}"
    );
    names
}

/// The Makefile's budget selector with `{}` replaced by `path`, ready for `sh`.
fn expanded_budget_selector(path: &str) -> String {
    let text = makefile();
    let selector = expand("FILE_TIMEOUT", &assignments(&text)).replace("$$", "$");
    // What survives expansion is shell. A `$(NAME)` still standing is a make
    // reference this could not follow, and handing it to `sh` would measure
    // something other than what the pass runs.
    let unresolved = selector
        .match_indices("$(")
        .any(|(at, _)| selector[at + 2..].starts_with(|c: char| c.is_ascii_uppercase()));
    assert!(
        !unresolved,
        "the budget selector still carries a make variable: {selector}"
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

// A file named in `WIDE_FILES` that no longer exists is not an error anywhere:
// the `case` alternative simply stops matching, and the file it was written for
// — under whatever name it now has — goes back to the narrow budget and starts
// dying on exit 124 under load. The counter-factual: rename one of the two
// heavy corpus files without touching the Makefile, and every gate still
// passes until a runner is slow enough.
#[test]
fn every_file_named_for_the_wider_budget_is_a_corpus_file() {
    let root = repo_root();
    for name in wide_file_names() {
        let path = root.join("tests/elle").join(&name);
        assert!(
            path.exists(),
            "the Makefile gives `{name}` the wider per-file budget, but \
             {} does not exist. The `case` alternative matches nothing, so \
             whatever that file is called now runs under TIMEOUT.",
            path.display()
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
    let text = makefile();
    let vars = assignments(&text);
    let wide = expand("WIDE_TIMEOUT", &vars);
    let narrow = expand("TIMEOUT", &vars);
    assert!(
        seconds(&wide) > seconds(&narrow),
        "WIDE_TIMEOUT is {wide}, which is not wider than TIMEOUT at {narrow}"
    );

    for name in wide_file_names() {
        let path = format!("tests/elle/{name}");
        assert_eq!(
            budget_for(&path),
            wide,
            "{path} is named in WIDE_FILES but the selector gives it {narrow}"
        );
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

// The wider budget is a backstop, not the deadline. A file that carries its own
// `deadline` reports on it — which request stalled, how long it waited — and
// that message is the reason to run the file at all. If the outer `timeout`
// fires first the message is never printed: the process dies on a signal and
// the gate reports exit 124. The counter-factual: set the wider budget below
// the in-file deadline and a genuine h2 stall is indistinguishable from a busy
// runner, which is the failure this whole mechanism exists to end.
#[test]
fn the_wider_budget_outlives_the_in_file_deadline_it_backstops() {
    let text = makefile();
    let budget = seconds(&expand("WIDE_TIMEOUT", &assignments(&text)));
    let root = repo_root();
    let mut checked = 0;
    for name in wide_file_names() {
        let source = fs::read_to_string(root.join("tests/elle").join(&name))
            .expect("read a file named for the wider budget");
        let Some((_, rest)) = source.split_once("(def deadline ") else {
            continue;
        };
        let deadline: u64 = rest
            .split(')')
            .next()
            .and_then(|n| n.trim().parse().ok())
            .unwrap_or_else(|| panic!("{name} declares a deadline this cannot read"));
        assert!(
            budget > deadline,
            "{name} gives itself {deadline} s to report a stall, and the pass \
             kills it at {budget} s. The kill lands first, so the file's own \
             diagnostic can never print."
        );
        checked += 1;
    }
    assert!(
        checked > 0,
        "no file named for the wider budget declares a deadline. Either the \
         declaration changed shape or the argument no longer applies — teach \
         this test the new shape rather than letting it pass by matching \
         nothing."
    );
}
