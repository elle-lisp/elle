// audited: 2026-10-26
//! Unit tests (`super` is the parent impl module): `run`'s exit status over directories and options.
//!
//! src/lint/README.md

use super::*;
use crate::scratch::ScratchDir;

/// A program whose one diagnostic is an error: `g` is never bound.
const UNDEFINED: &str = "(elle/epoch 15)\n(g)\n";

/// A program with no diagnostic at all.
const CLEAN: &str = "(elle/epoch 15)\n(+ 1 2)\n";

fn args(list: &[&str]) -> Vec<String> {
    list.iter().map(|s| s.to_string()).collect()
}

fn path(dir: &ScratchDir, name: &str) -> String {
    dir.join(name)
        .to_str()
        .expect("UTF-8 scratch path")
        .to_string()
}

fn write(dir: &ScratchDir, name: &str, content: &[u8]) -> String {
    let file = dir.join(name);
    std::fs::create_dir_all(file.parent().expect("parent")).expect("create parent");
    std::fs::write(&file, content).expect("write scratch file");
    path(dir, name)
}

/// The root of a scratch tree, as the argument `run` takes.
fn root(dir: &ScratchDir) -> String {
    path(dir, "")
}

#[test]
fn a_directory_lints_its_lisp_files() {
    // Counter-factual: the walk took only `.l` files, so this error went
    // unread and the run exited 0.
    let dir = ScratchDir::new("lint-dir-lisp");
    write(&dir, "a.lisp", UNDEFINED.as_bytes());
    assert_eq!(run(&args(&[&root(&dir)])), 1);
}

#[test]
fn a_directory_lints_a_nested_lisp_file() {
    let dir = ScratchDir::new("lint-dir-nested");
    write(&dir, "sub/x.lisp", UNDEFINED.as_bytes());
    assert_eq!(run(&args(&[&root(&dir)])), 1);
}

#[test]
fn a_directory_skips_a_dot_directory() {
    // The control for the nested case: the same error under `.git` is not read.
    let dir = ScratchDir::new("lint-dir-dot");
    write(&dir, ".git/x.lisp", UNDEFINED.as_bytes());
    write(&dir, "ok.lisp", CLEAN.as_bytes());
    assert_eq!(run(&args(&[&root(&dir)])), 0);
}

#[test]
fn a_directory_skips_a_file_that_is_not_lisp() {
    let dir = ScratchDir::new("lint-dir-ext");
    write(&dir, "notes.txt", UNDEFINED.as_bytes());
    write(&dir, "ok.lisp", CLEAN.as_bytes());
    assert_eq!(run(&args(&[&root(&dir)])), 0);
}

#[test]
fn an_unreadable_file_in_a_directory_exits_1() {
    // Not UTF-8, so `read_to_string` fails. Counter-factual: the walk dropped
    // the read error and the run exited 0.
    let dir = ScratchDir::new("lint-dir-unreadable");
    write(&dir, "bad.lisp", &[0xff, 0xfe, 0x28, 0x29]);
    assert_eq!(run(&args(&[&root(&dir)])), 1);
}

#[cfg(unix)]
#[test]
fn a_directory_that_cannot_be_listed_exits_1() {
    use std::os::unix::fs::PermissionsExt;
    let dir = ScratchDir::new("lint-dir-locked");
    write(&dir, "ok.lisp", CLEAN.as_bytes());
    let locked = dir.join("locked");
    std::fs::create_dir(&locked).expect("create locked dir");
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).expect("chmod");
    // The trap: root lists a mode-000 directory anyway, so under root this
    // test has nothing to observe.
    let listable = std::fs::read_dir(&locked).is_ok();
    let code = run(&args(&[&root(&dir)]));
    // Restore the mode before asserting, or the scratch directory cannot be
    // removed.
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o755)).expect("chmod");
    if listable {
        eprintln!("skipped: this user can list a mode-000 directory");
        return;
    }
    assert_eq!(code, 1);
}

#[test]
fn an_unknown_option_exits_1() {
    // Counter-factual: an argument starting with `-` was dropped, and the
    // clean file exited 0.
    let dir = ScratchDir::new("lint-opt-unknown");
    let file = write(&dir, "ok.lisp", CLEAN.as_bytes());
    assert_eq!(run(&args(&["--bogus", &file])), 1);
}

#[test]
fn level_with_no_value_exits_1() {
    let dir = ScratchDir::new("lint-opt-level");
    let file = write(&dir, "ok.lisp", CLEAN.as_bytes());
    assert_eq!(run(&args(&[&file, "--level"])), 1);
}

#[test]
fn format_with_no_value_exits_1() {
    let dir = ScratchDir::new("lint-opt-format");
    let file = write(&dir, "ok.lisp", CLEAN.as_bytes());
    assert_eq!(run(&args(&[&file, "--format"])), 1);
}

#[test]
fn a_clean_file_exits_0() {
    // The control for the option cases: the same file with no bad option.
    let dir = ScratchDir::new("lint-clean");
    let file = write(&dir, "ok.lisp", CLEAN.as_bytes());
    assert_eq!(run(&args(&[&file])), 0);
}
