//! Branch primitives.

use crate::helpers::{branch_to_value, get_repo, get_string, git_err, opts_get_bool};
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::{error_val, Value};

pub fn prim_git_branches(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/branches";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };

    let filter: Option<git2::BranchType> = if args.len() >= 2 {
        match args[1].as_keyword_name() {
            Some(kw) => match kw.as_str() {
                "local" => Some(git2::BranchType::Local),
                "remote" => Some(git2::BranchType::Remote),
                other => {
                    return (
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!("{}: filter must be :local or :remote, got :{}", name, other),
                        ),
                    );
                }
            },
            None => {
                return (
                    SIG_ERROR,
                    error_val(
                        "type-error",
                        format!(
                            "{}: filter must be a keyword, got {}",
                            name,
                            args[1].type_name()
                        ),
                    ),
                );
            }
        }
    } else {
        None
    };

    let branches = match repo.branches(filter) {
        Ok(b) => b,
        Err(e) => return git_err(name, e),
    };

    let mut result = Vec::new();
    for branch_result in branches {
        let (branch, kind) = match branch_result {
            Ok(b) => b,
            Err(e) => return git_err(name, e),
        };
        let branch_name = match branch.name() {
            Ok(Some(n)) => n.to_string(),
            Ok(None) => continue, // non-UTF-8 name: skip
            Err(e) => return git_err(name, e),
        };
        let oid = match branch.get().target() {
            Some(oid) => oid,
            None => continue, // symbolic ref with no target: skip
        };
        let upstream = branch
            .upstream()
            .ok()
            .and_then(|u| u.name().ok().flatten().map(|s| s.to_string()));
        result.push(branch_to_value(&branch_name, oid, kind, upstream));
    }
    (SIG_OK, elle::list(result))
}

pub fn prim_git_branch_create(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/branch-create";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let branch_name = match get_string(args, 1, name) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let target_str = if args.len() >= 3 {
        match get_string(args, 2, name) {
            Ok(s) => s,
            Err(e) => return e,
        }
    } else {
        "HEAD".to_string()
    };

    let target_obj = match repo.revparse_single(&target_str) {
        Ok(o) => o,
        Err(e) => return git_err(name, e),
    };
    let commit = match target_obj.peel_to_commit() {
        Ok(c) => c,
        Err(e) => return git_err(name, e),
    };
    let branch = match repo.branch(&branch_name, &commit, false) {
        Ok(b) => b,
        Err(e) => return git_err(name, e),
    };
    let oid = match branch.get().target() {
        Some(oid) => oid,
        None => {
            return (
                SIG_ERROR,
                error_val("git-error", format!("{}: branch target is symbolic", name)),
            )
        }
    };
    (SIG_OK, Value::string(oid.to_string()))
}

pub fn prim_git_branch_delete(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/branch-delete";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let branch_name = match get_string(args, 1, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut branch = match repo.find_branch(&branch_name, git2::BranchType::Local) {
        Ok(b) => b,
        Err(e) => return git_err(name, e),
    };
    match branch.delete() {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(e) => git_err(name, e),
    }
}

pub fn prim_git_checkout(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/checkout";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let refname = match get_string(args, 1, name) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let opts_val = if args.len() >= 3 { Some(args[2]) } else { None };
    let force = opts_get_bool(opts_val, "force");

    let obj = match repo.revparse_single(&refname) {
        Ok(o) => o,
        Err(e) => return git_err(name, e),
    };

    let mut checkout_opts = git2::build::CheckoutBuilder::new();
    if force {
        checkout_opts.force();
    }

    match repo.checkout_tree(&obj, Some(&mut checkout_opts)) {
        Ok(()) => {}
        Err(e) => return git_err(name, e),
    }

    // Determine whether to set HEAD symbolically (branch) or detach
    let set_head_result = if repo.find_branch(&refname, git2::BranchType::Local).is_ok() {
        repo.set_head(&format!("refs/heads/{}", refname))
    } else {
        repo.set_head_detached(obj.id())
    };

    match set_head_result {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(e) => git_err(name, e),
    }
}
