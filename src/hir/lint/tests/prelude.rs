// audited: 2026-10-27
//! Every prelude macro expands to bindings no lint rule reports.
//!
//! docs/cookbook/prelude-macros.md

use crate::lint::cli::{LintConfig, Linter};
use crate::pipeline::sources::PRELUDE;

/// One use of each prelude macro, keyed by the macro's name. Each use sits in a
/// lambda so that its free names are parameters, which no rule reads, and every
/// binding the use writes is read.
const USES: &[(&str, &str)] = &[
    ("defn", "(fn [] (defn g [x] x) (g 1))"),
    ("let*", "(fn [a] (let* [x a y x] y))"),
    ("->", "(fn [a] (-> a (+ 1) (* 2)))"),
    ("->>", "(fn [a] (->> a (+ 1)))"),
    ("as->", "(fn [a] (as-> a v (+ v 1) (* v 2)))"),
    ("some->", "(fn [a] (some-> a (get :k) (+ 1)))"),
    ("some->>", "(fn [a] (some->> a (+ 1)))"),
    ("when", "(fn [a] (when a 1))"),
    ("unless", "(fn [a] (unless a 1))"),
    ("default", "(fn [&named @x] (default x 1) x)"),
    ("yield", "(fn [a] (yield a))"),
    ("error", "(fn [a] (error a))"),
    ("assert", "(fn [a] (assert (= a 1) \"one\") (assert a))"),
    ("gate!", "(fn [a] (gate! a \"reason\" 1))"),
    ("try", "(fn [a] (try (a) (catch e e)))"),
    ("protect", "(fn [a] (protect (a)))"),
    ("defer", "(fn [a b] (defer (a) (b)))"),
    ("with", "(fn [open close] (with r (open) close r))"),
    ("with-temp-dir", "(fn [] (with-temp-dir d d))"),
    ("yield*", "(fn [co] (yield* co))"),
    (
        "ffi/defbind",
        "(fn [lib] (ffi/defbind abs lib \"abs\" :int [:int]) (abs 1))",
    ),
    ("each", "(fn [xs] (each x in xs x) (each y xs y))"),
    ("case", "(fn [a] (case a 1 :one 2 :two :other))"),
    ("if-let", "(fn [a] (if-let [x a] x 0))"),
    ("when-let", "(fn [a] (when-let [x a] x))"),
    ("when-ok", "(fn [a] (when-ok [v (a)] v))"),
    ("forever", "(fn [] (forever (break 1)))"),
    ("repeat", "(fn [n f] (repeat n (f)))"),
    ("apply", "(fn [f xs] (apply f 1 xs))"),
    (
        "ffi/with-stack",
        "(fn [] (ffi/with-stack [[p :int 42] [buf 64]] [p buf]))",
    ),
    (
        "with-allocator",
        "(fn [alloc f] (with-allocator alloc (f)))",
    ),
];

/// Every finding `elle lint` gives `source`, an analysis error included, as
/// `CODE message`. `Linter` builds a whole runtime, because a macro such as
/// `assert` calls a stdlib function while it expands.
fn findings(source: &str) -> Vec<String> {
    let mut linter = Linter::new(LintConfig::default());
    linter
        .lint_str(source, "<test>")
        .expect("lint_str reads a string");
    linter
        .diagnostics()
        .iter()
        .map(|d| format!("{} {}", d.code, d.message))
        .collect()
}

#[test]
fn no_prelude_macro_draws_a_lint_finding() {
    // Counter-factual: `each` bound its length as `@len` and never assigned it,
    // so every `each` drew two W003 warnings on a name the user never wrote.
    let drawn: Vec<(&str, Vec<String>)> = USES
        .iter()
        .map(|(name, source)| (*name, findings(source)))
        .filter(|(_, messages)| !messages.is_empty())
        .collect();
    assert!(drawn.is_empty(), "macros that drew findings: {drawn:#?}");
}

#[test]
fn every_prelude_macro_has_a_use() {
    // Counter-factual: a macro added to the prelude with no entry in USES would
    // pass the sweep above without ever being linted.
    let mut defined: Vec<&str> = PRELUDE
        .lines()
        .filter_map(|line| line.strip_prefix("(defmacro "))
        .filter_map(|rest| rest.split_whitespace().next())
        .collect();
    let mut used: Vec<&str> = USES.iter().map(|(name, _)| *name).collect();
    defined.sort_unstable();
    used.sort_unstable();
    assert_eq!(used, defined, "USES holds one use of each prelude macro");
}
