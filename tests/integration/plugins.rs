// audited: 2026-10-19
// The gate that keeps the `plugins/` submodule compiling, and the build of the
// plugins that the language suite loads.
//
// docs/analysis/plugins.md
//
// The submodule is a separate cargo workspace whose crates take `elle-plugin`
// by path, so a changed `elle_api!` declaration moves the SDK and the plugin
// together and the workspace either compiles or does not. Until the `Plugin
// Tests` job existed, nothing in CI checked the submodule out: #997 changed six
// declarations, stopped 17 plugins from compiling at about 90 call sites, and
// every gate stayed green (#1023). The argument, and why the ABI version guard
// cannot stand in for the job, are in docs/analysis/plugins.md § "The plugins
// job".
//
// These tests cover the halves the job cannot check for itself: that every
// plugin is inside the workspace the job compiles, that the list of artifacts
// it demands still describes the submodule, and that the demand actually fails
// when an artifact is absent. The last three tests hold the same for
// `SUITE_PLUGINS`, the plugins every suite job builds, and check that every
// suite job builds them before its first suite step.

use crate::common::{
    job_steps, lang_files, make_expand, make_steps, passes, recording_targets, runs_target,
    step_lines, suite, workflow_files, workflow_jobs,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Run one Makefile target with variable overrides, returning (success, output).
/// A command-line assignment beats the Makefile's own, which is what lets the
/// artifact assertion be driven both ways without a plugin build.
fn run_make(target: &str, overrides: &[&str]) -> (bool, String) {
    let out = Command::new("make")
        .current_dir(repo_root())
        .arg("--no-print-directory")
        .arg(target)
        .args(overrides)
        .env_remove("GITHUB_ACTIONS")
        .output()
        .unwrap_or_else(|e| panic!("run `make {target}`: {e}"));
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    (out.status.success(), text)
}

/// The plugins the artifact assertion must not demand. `make plugins-all`
/// compiles them and `make plugins` does not build them, so a `plugins-verify`
/// that named them would fail every portable build.
/// docs/analysis/plugins.md § "The plugins job" owns the split.
const NOT_PORTABLE: &[&str] = &["polars", "arrow", "vulkan", "egui", "wayland"];

/// The package name a plugin directory declares, or `None` when the directory
/// holds no crate.
fn package_name(dir: &str) -> Option<String> {
    let text = fs::read_to_string(repo_root().join("plugins").join(dir).join("Cargo.toml")).ok()?;
    let line = text
        .lines()
        .find(|l| l.trim_start().starts_with("name = "))?;
    Some(line.split('"').nth(1)?.to_string())
}

/// The directories under `plugins/` that hold a crate.
fn plugin_directories() -> Vec<String> {
    let root = repo_root().join("plugins");
    let mut dirs: Vec<String> = fs::read_dir(&root)
        .unwrap_or_else(|e| panic!("read {}: {e}", root.display()))
        .filter_map(|entry| entry.ok())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|dir| root.join(dir).join("Cargo.toml").is_file())
        .collect();
    dirs.sort();
    dirs
}

/// The `members` list of `plugins/Cargo.toml`'s `[workspace]` table.
fn workspace_members() -> Vec<String> {
    let path = repo_root().join("plugins/Cargo.toml");
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let (_, rest) = text
        .split_once("members = [")
        .unwrap_or_else(|| panic!("{} declares no `members` list", path.display()));
    let (inner, _) = rest
        .split_once(']')
        .unwrap_or_else(|| panic!("{}'s `members` list does not close", path.display()));
    // The list is one quoted directory name per line, so the odd fields of a
    // split on the quote character are the names.
    inner
        .split('"')
        .skip(1)
        .step_by(2)
        .map(str::to_string)
        .collect()
}

// The job compiles the workspace, and `plugins/Cargo.toml` decides what that
// is. A plugin directory the `members` list does not name is compiled by
// nothing: `cargo build` over the workspace never descends into it, the
// artifact assertion cannot demand a `.so` that no target produces, and the
// corpus file for it exits 0 on the import it could not resolve. Three gates
// agree, and none of them looked.
//
// The trap: cargo reports an unlisted directory only when a build STARTS
// inside it ("current package believes it's in a workspace when it's not"), and
// CI never starts a build there. From the workspace root the directory is
// invisible, not an error.
//
// The counter-factual: drop `csv` from `members`, and `make plugins-all`,
// `make plugins-verify` and the plugin corpus all stay green over a plugin
// that no longer compiles at all.
#[test]
fn every_plugin_directory_is_a_workspace_member() {
    if !repo_root().join("plugins/Cargo.toml").exists() {
        eprintln!("SKIPPED: the `plugins/` submodule is not checked out");
        return;
    }

    let members = workspace_members();
    // A parse that returned nothing would assert nothing. The workspace has
    // held around twenty-five crates since the submodule was split out.
    assert!(
        members.len() > 10,
        "plugins/Cargo.toml names only {members:?} as workspace members; the \
         parse of the `members` list is broken, not the manifest"
    );

    let unlisted: Vec<String> = plugin_directories()
        .into_iter()
        .filter(|dir| !members.contains(dir))
        .collect();
    assert!(
        unlisted.is_empty(),
        "these plugin directories hold a crate that plugins/Cargo.toml's \
         `members` list does not name, so `make plugins-all` never compiles \
         them and an ABI break in them reaches no gate: {unlisted:?}"
    );

    let empty: Vec<&String> = members
        .iter()
        .filter(|dir| package_name(dir).is_none())
        .collect();
    assert!(
        empty.is_empty(),
        "plugins/Cargo.toml names these members, which declare no package: \
         {empty:?}"
    );
}

// The artifact list is derived, not written down: the Makefile reads `PORTABLE`
// back out of the submodule's own make and turns each package into the cdylib
// name cargo emits for it. That derivation has two ways to go wrong quietly —
// the read can return nothing, and the name mapping can stop matching the
// crates — and either one makes the assertion pass while demanding nothing.
//
// The counter-factual: a plugin crate that sets `[lib] name` produces a `.so`
// under a name this mapping never predicts. The assertion then demands a file
// that is never built and fails every run, which is loud; the mapping breaking
// the other way, on a package the list stops naming, is the silent half this
// test catches.
#[test]
fn the_demanded_artifacts_describe_the_submodule() {
    if !repo_root().join("plugins/Makefile").exists() {
        eprintln!("SKIPPED: the `plugins/` submodule is not checked out");
        return;
    }

    let (ok, listed) = run_make("print-PORTABLE_SO", &[]);
    assert!(ok, "`make print-PORTABLE_SO` failed:\n{listed}");
    let artifacts: Vec<&str> = listed.split_whitespace().collect();

    // A read that returned nothing would assert nothing. The portable set has
    // held around twenty plugins since the submodule was split out.
    assert!(
        artifacts.len() > 10,
        "the Makefile demands only {} plugin artifacts; the read of \
         `PORTABLE` out of plugins/Makefile is broken, not the submodule: {listed}",
        artifacts.len()
    );

    for artifact in &artifacts {
        let stem = artifact
            .strip_prefix("target/release/libelle_")
            .and_then(|s| s.strip_suffix(".so"))
            .unwrap_or_else(|| panic!("`{artifact}` is not a `target/release/libelle_*.so` path"));

        // Cargo names a cdylib after the package, with `-` as `_`, unless the
        // crate overrides it with `[lib] name`. The directory is the package
        // name without the `elle-` prefix.
        let dir = stem.replace('_', "-");
        let package = package_name(&dir).unwrap_or_else(|| {
            panic!("`{artifact}` names plugins/{dir}, which declares no package")
        });
        assert_eq!(
            package.replace('-', "_"),
            format!("elle_{stem}"),
            "plugins/{dir} is package `{package}`, which cargo builds as \
             something other than `{artifact}`"
        );

        let manifest =
            fs::read_to_string(repo_root().join("plugins").join(&dir).join("Cargo.toml")).unwrap();
        assert!(
            manifest.contains("crate-type = [\"cdylib\"]"),
            "plugins/{dir} is not a cdylib, so it produces no `{artifact}`"
        );
        assert!(
            !manifest.contains("[lib]\nname = "),
            "plugins/{dir} overrides its lib name, so `{artifact}` is a guess"
        );
    }

    for excluded in NOT_PORTABLE {
        let path = format!("target/release/libelle_{}.so", excluded.replace('-', "_"));
        assert!(
            !artifacts.contains(&path.as_str()),
            "the assertion demands `{path}`, which the portable build does not \
             produce; see docs/analysis/plugins.md § \"The plugins job\""
        );
    }
}

// The assertion has to fail on a missing artifact, because nothing downstream
// of it will. Every plugins/tests/*.lisp imports its `.so` under `protect` and
// exits 0 when the import fails, so a plugin that did not build makes its own
// test report success — run from a directory where the paths did not resolve,
// 13 of 19 files reported `ok` having executed nothing.
//
// The counter-factual this replaces: `make plugins` followed by the corpus, and
// nothing between them. A plugin dropped from the build took its test's
// coverage with it and the job stayed green.
#[test]
fn the_artifact_assertion_fails_on_a_missing_plugin() {
    let (ok, output) = run_make(
        "plugins-verify",
        &["PORTABLE_SO=target/release/libelle_never_built.so"],
    );
    assert!(
        !ok,
        "`plugins-verify` passed with an artifact that does not exist:\n{output}"
    );
    assert!(
        output.contains("libelle_never_built.so"),
        "the failure does not name the missing artifact, so the reader has to \
         bisect the plugin set to find it:\n{output}"
    );

    // The same target over a file that is present must pass, or the test above
    // proves only that the target always fails.
    let (ok, output) = run_make("plugins-verify", &["PORTABLE_SO=Makefile"]);
    assert!(
        ok,
        "`plugins-verify` failed over an artifact that exists:\n{output}"
    );
}

// An empty list satisfies "every artifact is present" vacuously, and that is
// exactly the state of a tree whose submodule was never checked out: the read
// of `PORTABLE` returns nothing and the assertion greens. The gate has to
// report the un-checked-out submodule instead of passing over it.
#[test]
fn the_artifact_assertion_rejects_an_empty_set() {
    let (ok, output) = run_make("plugins-verify", &["PORTABLE_SO="]);
    assert!(
        !ok,
        "`plugins-verify` passed while demanding no artifacts at all, which is \
         what a missing `plugins/` checkout looks like:\n{output}"
    );
}

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
                package_name(name).as_deref(),
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
