// audited: 2026-10-21
// The plugins the language suite loads: SUITE_PLUGINS names each one, its build is asserted, and every suite job builds them.
//
// docs/analysis/plugins.md
//
// A language-suite file that imports a plugin gates itself when the import
// fails, so a build without the plugin skips the file and stays green. These
// tests hold `SUITE_PLUGINS` to what the suite imports, hold its artifact
// assertion to failing on a missing plugin, and check that every suite job
// builds the plugins before its first suite step.

use crate::common::{
    job_steps, lang_files, make_expand, make_steps, passes, plugin_package, recording_targets,
    repo_root, run_make, runs_target, step_lines, suite, workflow_files, workflow_jobs,
};
use std::collections::BTreeSet;
use std::fs;

/// The `X` of every `(import "plugin/X")` in a language-suite file.
fn plugins_the_language_suite_imports() -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for path in suite("tests/lang") {
        let text = fs::read_to_string(repo_root().join(&path))
            .unwrap_or_else(|e| panic!("read {path}: {e}"));
        for rest in text.split("(import \"plugin/").skip(1) {
            if let Some((name, _)) = rest.split_once('"') {
                out.insert(name.to_string());
            }
        }
    }
    out
}

// A language-suite file that imports a plugin gates itself when the import
// fails, and the suite jobs build only what `SUITE_PLUGINS` names. A plugin
// missing from that list is built by no suite job, so its file skips on every
// build and the run stays green. A name the suite no longer imports costs every
// suite job a build for nothing. docs/analysis/plugins.md owns the argument.
//
// The counter-factual: this was the state of websocket.lisp, whose loopback
// test ran in no suite job because no suite job built `plugin/hash`.
#[test]
fn every_plugin_the_language_suite_imports_is_a_suite_plugin() {
    let imported = plugins_the_language_suite_imports();
    assert!(
        imported.contains("hash"),
        "the read of the language suite's plugin imports found only \
         {imported:?}; websocket.lisp imports plugin/hash, so the read is broken"
    );
    let built: BTreeSet<String> = make_expand("SUITE_PLUGINS")
        .split_whitespace()
        .map(str::to_string)
        .collect();

    let unbuilt: Vec<&String> = imported.difference(&built).collect();
    assert!(
        unbuilt.is_empty(),
        "language-suite files import these plugins, which `SUITE_PLUGINS` does \
         not name, so no suite job builds them and their files skip on every \
         build: {unbuilt:?}"
    );
    let unused: Vec<&String> = built.difference(&imported).collect();
    assert!(
        unused.is_empty(),
        "`SUITE_PLUGINS` names these plugins, which no language-suite file \
         imports, so every suite job builds them for nothing: {unused:?}"
    );

    if repo_root().join("plugins/Cargo.toml").exists() {
        for name in &built {
            assert_eq!(
                plugin_package(name).as_deref(),
                Some(format!("elle-{name}").as_str()),
                "`SUITE_PLUGINS` names `{name}`, which is not the package \
                 plugins/{name}/Cargo.toml declares"
            );
        }
    }
}

// The suite plugins get the artifact assertion `plugins-verify` gives the
// portable set, for the same reason: a suite file that cannot import its
// plugin skips, so the build step is the only thing that can report a plugin
// that did not build. The assertion must fail on a missing artifact and on an
// empty list, and pass over a file that exists.
#[test]
fn the_suite_plugin_assertion_fails_on_a_missing_plugin() {
    let (ok, output) = run_make(
        "suite-plugins-verify",
        &["SUITE_PLUGIN_SO=target/release/libelle_never_built.so"],
    );
    assert!(
        !ok,
        "`suite-plugins-verify` passed with an artifact that does not \
         exist:\n{output}"
    );
    assert!(
        output.contains("libelle_never_built.so"),
        "the failure does not name the missing artifact:\n{output}"
    );

    let (ok, output) = run_make("suite-plugins-verify", &["SUITE_PLUGIN_SO="]);
    assert!(
        !ok,
        "`suite-plugins-verify` passed while demanding no artifacts:\n{output}"
    );

    let (ok, output) = run_make("suite-plugins-verify", &["SUITE_PLUGIN_SO=Makefile"]);
    assert!(
        ok,
        "`suite-plugins-verify` failed over an artifact that exists:\n{output}"
    );
}

/// Does this step run a language-suite file or the documents? It runs `make
/// doctest`, or a target with a pass that deals out a file of the language
/// suite, as `make --dry-run` spells the pass.
fn runs_language_suite(step: &str, recording: &BTreeSet<String>, lang: &BTreeSet<String>) -> bool {
    make_steps(step).into_iter().any(|(target, vars)| {
        if target == "doctest" {
            return true;
        }
        if !recording.contains(&target) {
            return false;
        }
        let vars: Vec<&str> = vars.iter().map(String::as_str).collect();
        passes(&target, &vars)
            .iter()
            .any(|pass| !pass.files.is_disjoint(lang))
    })
}

// A language-suite file that imports a plugin gates itself when the import
// fails, and a document gates a plugin example the same way, so a job that
// never built the plugin skips them and stays green. The plugins live in the
// `plugins/` submodule, which a job has to check out before it can build
// them. The argument is in docs/analysis/plugins.md.
//
// The trap: the language suite runs in more targets than the ones named for
// it. `make smoke-eager`, `make smoke-charge` and `make smoke-scrub` run its
// files too, so a list of target names misses a job whose first suite step is
// one of those. The test reads each pass's files off `make --dry-run` instead.
//
// The counter-factual: this was every suite job before the step existed, and
// the loopback test in websocket.lisp ran nowhere. Drop the step from one job,
// and that build skips every plugin test with nothing going red.
#[test]
fn every_suite_job_builds_the_suite_plugins_before_its_first_suite_step() {
    let recording = recording_targets();
    let lang = lang_files();
    let mut checked: Vec<String> = Vec::new();
    for (file, text) in workflow_files() {
        for (name, body) in workflow_jobs(&text) {
            let steps: Vec<String> = job_steps(&body)
                .iter()
                .map(|s| step_lines(s).join("\n"))
                .collect();
            let Some(first) = steps
                .iter()
                .position(|s| runs_language_suite(s, &recording, &lang))
            else {
                continue;
            };
            let checkout = steps
                .iter()
                .position(|s| s.contains("git submodule update --init plugins"));
            let build = steps.iter().position(|s| runs_target(s, "suite-plugins"));
            assert!(
                matches!((checkout, build), (Some(c), Some(b)) if c < b && b < first),
                "job `{name}` in {file} runs the language suite or the doctests \
                 without first checking out the plugins submodule and running \
                 `make suite-plugins`, so its plugin tests skip: checkout at \
                 step {checkout:?}, build at step {build:?}, first suite step {first}"
            );
            checked.push(format!("{file}:{name}"));
        }
    }
    // A parse that matched nothing would assert nothing. Eight pull-request
    // jobs and the merge queue run the suite or the doctests.
    assert!(
        checked.len() > 6,
        "found only {checked:?} as suite jobs; the parse is broken, not the \
         workflows"
    );
}
