//! Repository lifecycle and HEAD primitives.

use crate::helpers::{get_repo, get_string, git_err, repo_state_to_keyword};
use elle::value::fiber::{SignalBits, SIG_OK};
use elle::value::{TableKey, Value};
use git2::Repository;
use std::collections::BTreeMap;

pub fn prim_git_open(args: &[Value]) -> (SignalBits, Value) {
    let path = match get_string(args, 0, "git/open") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match Repository::open(&path) {
        Ok(repo) => (SIG_OK, Value::external("git/repo", repo)),
        Err(e) => git_err("git/open", e),
    }
}

pub fn prim_git_init(args: &[Value]) -> (SignalBits, Value) {
    let path = match get_string(args, 0, "git/init") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match Repository::init(&path) {
        Ok(repo) => (SIG_OK, Value::external("git/repo", repo)),
        Err(e) => git_err("git/init", e),
    }
}

pub fn prim_git_clone(args: &[Value]) -> (SignalBits, Value) {
    let url = match get_string(args, 0, "git/clone") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let path = match get_string(args, 1, "git/clone") {
        Ok(s) => s,
        Err(e) => return e,
    };
    match Repository::clone(&url, &path) {
        Ok(repo) => (SIG_OK, Value::external("git/repo", repo)),
        Err(e) => git_err("git/clone", e),
    }
}

pub fn prim_git_path(args: &[Value]) -> (SignalBits, Value) {
    let repo = match get_repo(args, "git/path") {
        Ok(r) => r,
        Err(e) => return e,
    };
    let path_str = repo.path().to_string_lossy().to_string();
    (SIG_OK, Value::string(path_str))
}

pub fn prim_git_workdir(args: &[Value]) -> (SignalBits, Value) {
    let repo = match get_repo(args, "git/workdir") {
        Ok(r) => r,
        Err(e) => return e,
    };
    match repo.workdir() {
        Some(p) => (SIG_OK, Value::string(p.to_string_lossy().to_string())),
        None => (SIG_OK, Value::NIL),
    }
}

pub fn prim_git_bare(args: &[Value]) -> (SignalBits, Value) {
    let repo = match get_repo(args, "git/bare?") {
        Ok(r) => r,
        Err(e) => return e,
    };
    (SIG_OK, Value::bool(repo.is_bare()))
}

pub fn prim_git_state(args: &[Value]) -> (SignalBits, Value) {
    let repo = match get_repo(args, "git/state") {
        Ok(r) => r,
        Err(e) => return e,
    };
    (SIG_OK, repo_state_to_keyword(repo.state()))
}

pub fn prim_git_head(args: &[Value]) -> (SignalBits, Value) {
    let repo = match get_repo(args, "git/head") {
        Ok(r) => r,
        Err(e) => return e,
    };
    let head = match repo.head() {
        Ok(h) => h,
        Err(e) => return git_err("git/head", e),
    };
    let name_val = match head.name() {
        Some(n) => Value::string(n.to_string()),
        None => Value::NIL,
    };
    let oid_val = match head.target() {
        Some(oid) => Value::string(oid.to_string()),
        None => Value::NIL,
    };
    let symbolic = head.is_branch();
    let mut fields = BTreeMap::new();
    fields.insert(TableKey::Keyword("name".into()), name_val);
    fields.insert(TableKey::Keyword("oid".into()), oid_val);
    fields.insert(TableKey::Keyword("symbolic".into()), Value::bool(symbolic));
    (SIG_OK, Value::struct_from(fields))
}

pub fn prim_git_resolve(args: &[Value]) -> (SignalBits, Value) {
    let repo = match get_repo(args, "git/resolve") {
        Ok(r) => r,
        Err(e) => return e,
    };
    let refname = match get_string(args, 1, "git/resolve") {
        Ok(s) => s,
        Err(e) => return e,
    };
    let obj = match repo.revparse_single(&refname) {
        Ok(o) => o,
        Err(e) => return git_err("git/resolve", e),
    };
    (SIG_OK, Value::string(obj.id().to_string()))
}
