// audited: 2026-09-17
// Where `elle test` keeps its session DB when no `--db` names one: a state
// directory, never the cache that a reboot may clear.
//
// docs/test-store.md
//
// The counter-factual: a runner that reads `ELLE_CACHE` first puts run history
// on whatever that variable points at. On a development box that is a tmpfs,
// so every run recorded before a reboot is gone — and the history is the whole
// point of the store.

use std::path::PathBuf;
use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// Run `elle test` over a trivial passing form with no `--db`, so the runner
/// derives the store location itself. Every variable the derivation reads is
/// cleared first, then `vars` sets the ones this case is about.
fn run_derived(dir: &crate::common::ScratchDir, vars: &[(&str, &str)]) -> std::process::Output {
    let fixture = dir.join("pass.lisp");
    std::fs::write(&fixture, "(assert true \"ok\")\n").expect("write fixture");

    let mut cmd = Command::new(elle_binary());
    cmd.args(["test"])
        .arg(&fixture)
        .args(["--timeout", "30000"])
        .env_remove("RUST_MIN_STACK")
        .env_remove("ELLE_STATE")
        .env_remove("XDG_STATE_HOME")
        .env_remove("ELLE_CACHE")
        .env_remove("HOME");
    for (key, value) in vars {
        cmd.env(key, value);
    }
    cmd.output().expect("run elle test")
}

/// Assert the run gated green and left a database, a CAS, and a scratch
/// directory at `db`.
fn assert_store_at(out: &std::process::Output, db: PathBuf) {
    assert!(
        out.status.success(),
        "the run should gate green; stderr:\n{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        db.is_file(),
        "expected the session DB at {}; stderr:\n{}",
        db.display(),
        String::from_utf8_lossy(&out.stderr)
    );
    let parent = db.parent().expect("the DB has a parent");
    assert!(
        parent.join("cas").is_dir(),
        "the CAS belongs beside the DB, at {}",
        parent.join("cas").display()
    );
    assert!(
        parent.join("scratch").is_dir(),
        "the scratch directory belongs beside the DB, at {}",
        parent.join("scratch").display()
    );
}

#[test]
fn elle_state_names_the_store() {
    let dir = crate::common::ScratchDir::new("state-dir-explicit");
    let state = dir.join("state");
    let out = run_derived(&dir, &[("ELLE_STATE", state.to_str().expect("utf-8 path"))]);
    assert_store_at(&out, state.join("elle-tests.db"));
}

#[test]
fn xdg_state_home_gets_an_elle_directory() {
    let dir = crate::common::ScratchDir::new("state-dir-xdg");
    let xdg = dir.join("xdg");
    let out = run_derived(&dir, &[("XDG_STATE_HOME", xdg.to_str().expect("utf-8 path"))]);
    assert_store_at(&out, xdg.join("elle").join("elle-tests.db"));
}

#[test]
fn home_falls_back_to_the_xdg_default() {
    let dir = crate::common::ScratchDir::new("state-dir-home");
    let home = dir.join("home");
    let out = run_derived(&dir, &[("HOME", home.to_str().expect("utf-8 path"))]);
    assert_store_at(
        &out,
        home.join(".local").join("state").join("elle").join("elle-tests.db"),
    );
}

/// The defect this closes: run history followed `ELLE_CACHE`. A state variable
/// outranks the cache, and the cache keeps nothing.
#[test]
fn the_cache_does_not_hold_run_history() {
    let dir = crate::common::ScratchDir::new("state-dir-cache-loses");
    let home = dir.join("home");
    let cache = dir.join("cache");
    std::fs::create_dir_all(&cache).expect("create the cache directory");
    let out = run_derived(
        &dir,
        &[
            ("HOME", home.to_str().expect("utf-8 path")),
            ("ELLE_CACHE", cache.to_str().expect("utf-8 path")),
        ],
    );
    assert_store_at(
        &out,
        home.join(".local").join("state").join("elle").join("elle-tests.db"),
    );
    assert!(
        !cache.join("elle-tests.db").exists(),
        "the cache must hold no run history, found {}",
        cache.join("elle-tests.db").display()
    );
}

/// With no state variable and no home, the cache is the last place left that
/// names a directory.
#[test]
fn the_cache_is_the_last_resort() {
    let dir = crate::common::ScratchDir::new("state-dir-cache-last");
    let cache = dir.join("cache-only");
    let out = run_derived(&dir, &[("ELLE_CACHE", cache.to_str().expect("utf-8 path"))]);
    assert_store_at(&out, cache.join("elle-tests.db"));
}

/// An empty variable names no directory, so the derivation reads past it. A
/// runner that took it literally would write to `/elle-tests.db`.
#[test]
fn an_empty_variable_counts_as_unset() {
    let dir = crate::common::ScratchDir::new("state-dir-empty");
    let home = dir.join("home");
    let out = run_derived(
        &dir,
        &[
            ("ELLE_STATE", ""),
            ("XDG_STATE_HOME", ""),
            ("HOME", home.to_str().expect("utf-8 path")),
        ],
    );
    assert_store_at(
        &out,
        home.join(".local").join("state").join("elle").join("elle-tests.db"),
    );
}
