// audited: 2026-10-15
// Where `elle` caches its compiled standard library when no flag names a directory.
//
// docs/config.md
// docs/impl/stdlib-cache.md
//
// The counter-factual: the cache followed `TMPDIR` or `TMP`, and with neither
// set it was off. A start without them compiled the standard library again,
// and nothing failed: the start was only slower. Each test here clears every
// variable the lookup reads, then sets the ones its case is about.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Run `elle -e (+ 1 2)` with every variable the cache lookup reads cleared,
/// then `vars` set, and assert that it answered.
fn run_elle(vars: &[(&str, &Path)]) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_elle"));
    cmd.args(["-e", "(+ 1 2)"])
        .env_remove("ELLE_CACHE")
        .env_remove("XDG_CACHE_HOME")
        .env_remove("HOME")
        .env_remove("TMPDIR")
        .env_remove("TMP");
    for (key, value) in vars {
        cmd.env(key, value);
    }
    let out = cmd.output().expect("run elle");
    assert!(
        out.status.success(),
        "elle -e (+ 1 2) failed; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
}

/// The stdlib cache files under the cache directory `dir`.
fn cached_stdlib(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir.join("stdlib-cache")) else {
        return Vec::new();
    };
    entries
        .map(|entry| entry.expect("a stdlib-cache entry").path())
        .filter(|path| path.extension().is_some_and(|e| e == "bin"))
        .collect()
}

/// A Linux session often sets neither `TMPDIR` nor `TMP`.
#[test]
fn with_no_temp_variable_the_stdlib_cache_lands_under_home() {
    let dir = crate::common::ScratchDir::new("cache-dir-home");
    let home = dir.join("home");
    run_elle(&[("HOME", &home)]);
    let cache = home.join(".cache").join("elle");
    assert!(
        !cached_stdlib(&cache).is_empty(),
        "with HOME set and no temp variable, the start must cache its stdlib \
         under {}",
        cache.display()
    );
}

#[test]
fn the_temp_directory_does_not_choose_the_cache() {
    let dir = crate::common::ScratchDir::new("cache-dir-tmp");
    let home = dir.join("home");
    let tmp = dir.join("tmp");
    std::fs::create_dir_all(&tmp).expect("create the temp directory");
    run_elle(&[("HOME", &home), ("TMPDIR", &tmp)]);
    assert!(
        !cached_stdlib(&home.join(".cache").join("elle")).is_empty(),
        "HOME outranks TMPDIR, so the stdlib cache belongs under HOME"
    );
    assert!(
        cached_stdlib(&tmp.join("elle-cache")).is_empty(),
        "the stdlib cache must not follow TMPDIR"
    );
}

#[test]
fn xdg_cache_home_names_the_cache() {
    let dir = crate::common::ScratchDir::new("cache-dir-xdg");
    let xdg = dir.join("xdg");
    run_elle(&[("HOME", &dir.join("home")), ("XDG_CACHE_HOME", &xdg)]);
    assert!(
        !cached_stdlib(&xdg.join("elle")).is_empty(),
        "the stdlib cache belongs under {}",
        xdg.join("elle").display()
    );
}
