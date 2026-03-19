//! Remote primitives.

use crate::helpers::{get_repo, get_string, git_err};
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::{error_val, TableKey, Value};
use std::collections::BTreeMap;

pub fn prim_git_remotes(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/remotes";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let remotes = match repo.remotes() {
        Ok(r) => r,
        Err(e) => return git_err(name, e),
    };
    let names: Vec<Value> = remotes
        .iter()
        .filter_map(|n| n.map(|s| Value::string(s.to_string())))
        .collect();
    (SIG_OK, elle::list(names))
}

pub fn prim_git_remote_info(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/remote-info";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let remote_name = match get_string(args, 1, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let remote = match repo.find_remote(&remote_name) {
        Ok(r) => r,
        Err(e) => return git_err(name, e),
    };
    let url_val = match remote.url() {
        Some(u) => Value::string(u.to_string()),
        None => Value::NIL,
    };
    let push_url_val = match remote.pushurl() {
        Some(u) => Value::string(u.to_string()),
        None => Value::NIL,
    };
    let mut fields = BTreeMap::new();
    fields.insert(TableKey::Keyword("name".into()), Value::string(remote_name));
    fields.insert(TableKey::Keyword("url".into()), url_val);
    fields.insert(TableKey::Keyword("push-url".into()), push_url_val);
    (SIG_OK, Value::struct_from(fields))
}

pub fn prim_git_fetch(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/fetch";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let remote_name = match get_string(args, 1, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut remote = match repo.find_remote(&remote_name) {
        Ok(r) => r,
        Err(e) => return git_err(name, e),
    };
    let empty: &[&str] = &[];
    match remote.fetch(empty, None, None) {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(e) => git_err(name, e),
    }
}

pub fn prim_git_push(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/push";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let remote_name = match get_string(args, 1, name) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let refspecs: Vec<String> = if args.len() >= 3 {
        match args[2].as_array() {
            Some(arr) => {
                let mut specs = Vec::new();
                for item in arr.iter() {
                    match item.with_string(|s| s.to_string()) {
                        Some(s) => specs.push(s),
                        None => {
                            return (
                                SIG_ERROR,
                                error_val(
                                    "type-error",
                                    format!("{}: refspecs must be strings", name),
                                ),
                            )
                        }
                    }
                }
                specs
            }
            None => {
                return (
                    SIG_ERROR,
                    error_val("type-error", format!("{}: refspecs must be an array", name)),
                )
            }
        }
    } else {
        // Default: push current branch
        let head = match repo.head() {
            Ok(h) => h,
            Err(e) => return git_err(name, e),
        };
        if head.is_branch() {
            let branch_name = match head.shorthand() {
                Some(n) => n.to_string(),
                None => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "git-error",
                            format!("{}: could not get current branch name", name),
                        ),
                    )
                }
            };
            vec![format!(
                "refs/heads/{}:refs/heads/{}",
                branch_name, branch_name
            )]
        } else {
            return (
                SIG_ERROR,
                error_val(
                    "git-error",
                    format!("{}: HEAD is detached; provide explicit refspecs", name),
                ),
            );
        }
    };

    let mut remote = match repo.find_remote(&remote_name) {
        Ok(r) => r,
        Err(e) => return git_err(name, e),
    };
    let refspec_refs: Vec<&str> = refspecs.iter().map(|s| s.as_str()).collect();
    match remote.push(&refspec_refs, None) {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(e) => git_err(name, e),
    }
}
