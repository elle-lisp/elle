// The gate that keeps the `plugins/` submodule compiling against this tree.
//
// The submodule is a separate cargo workspace whose crates take `elle-plugin`
// by path, so a changed `elle_api!` declaration moves the SDK and the plugin
// together and the workspace either compiles or does not. Until the `Plugin
// Tests` job existed, nothing in CI checked the submodule out: #997 changed six
// declarations, stopped 17 plugins from compiling at about 90 call sites, and
// every gate stayed green (#1023). The argument, and why the ABI version guard
// cannot stand in for the job, are in docs/analysis/ci.md § "The plugins job".
//
// These tests cover the halves the job cannot check for itself: that every
// plugin is inside the workspace the job compiles, that the list of artifacts
// it demands still describes the submodule, and that the demand actually fails
// when an artifact is absent.

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
/// docs/analysis/ci.md § "The plugins job" owns the split.
const NOT_PORTABLE: &[&str] = &["polars", "arrow", "vulkan", "egui", "wayland"];

/// The package name a plugin directory declares, or `None` when the directory
/// holds no crate.
fn package_name(dir: &str) -> Option<String> {
    let text = fs::read_to_string(repo_root().join("plugins").join(dir).join("Cargo.toml")).ok()?;
    let line = text.lines().find(|l| l.trim_start().starts_with("name = "))?;
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
            .unwrap_or_else(|| {
                panic!("`{artifact}` is not a `target/release/libelle_*.so` path")
            });

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
             produce; see docs/analysis/ci.md § \"The plugins job\""
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
