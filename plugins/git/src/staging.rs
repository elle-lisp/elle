//! Status and index (staging) primitives.

use crate::helpers::{extract_paths, get_repo, git_err, status_to_keyword};
use elle::value::fiber::{SignalBits, SIG_OK};
use elle::value::{TableKey, Value};
use std::collections::BTreeMap;

pub fn prim_git_status(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/status";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let statuses = match repo.statuses(None) {
        Ok(s) => s,
        Err(e) => return git_err(name, e),
    };
    let mut result = Vec::new();
    for entry in statuses.iter() {
        let path = entry.path().unwrap_or("").to_string();
        let status = entry.status();
        let index_kw = status_to_keyword(status, true);
        let workdir_kw = status_to_keyword(status, false);
        let mut fields = BTreeMap::new();
        fields.insert(TableKey::Keyword("path".into()), Value::string(path));
        fields.insert(TableKey::Keyword("index".into()), index_kw);
        fields.insert(TableKey::Keyword("workdir".into()), workdir_kw);
        result.push(Value::struct_from(fields));
    }
    (SIG_OK, elle::list(result))
}

pub fn prim_git_add(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/add";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let paths = match extract_paths(args[1], name) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut index = match repo.index() {
        Ok(i) => i,
        Err(e) => return git_err(name, e),
    };
    for path in &paths {
        match index.add_path(path) {
            Ok(()) => {}
            Err(e) => return git_err(name, e),
        }
    }
    match index.write() {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(e) => git_err(name, e),
    }
}

pub fn prim_git_remove(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/remove";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let paths = match extract_paths(args[1], name) {
        Ok(p) => p,
        Err(e) => return e,
    };
    let mut index = match repo.index() {
        Ok(i) => i,
        Err(e) => return git_err(name, e),
    };
    for path in &paths {
        match index.remove_path(path) {
            Ok(()) => {}
            Err(e) => return git_err(name, e),
        }
    }
    match index.write() {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(e) => git_err(name, e),
    }
}

pub fn prim_git_add_all(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/add-all";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let mut index = match repo.index() {
        Ok(i) => i,
        Err(e) => return git_err(name, e),
    };
    match index.add_all(["*"].iter(), git2::IndexAddOption::DEFAULT, None) {
        Ok(()) => {}
        Err(e) => return git_err(name, e),
    }
    match index.update_all(["*"].iter(), None) {
        Ok(()) => {}
        Err(e) => return git_err(name, e),
    }
    match index.write() {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(e) => git_err(name, e),
    }
}
