//! Commit primitives.

use crate::helpers::{
    commit_to_value, get_repo, get_string, git_err, make_signature, opts_get, opts_get_int,
    opts_get_string,
};
use elle::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use elle::value::{error_val, Value};

pub fn prim_git_commit_info(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/commit-info";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let oid_str = match get_string(args, 1, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let oid = match git2::Oid::from_str(&oid_str) {
        Ok(o) => o,
        Err(e) => return git_err(name, e),
    };
    let commit = match repo.find_commit(oid) {
        Ok(c) => c,
        Err(e) => return git_err(name, e),
    };
    (SIG_OK, commit_to_value(&commit))
}

pub fn prim_git_log(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/log";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };

    let opts_val = if args.len() >= 2 { Some(args[1]) } else { None };
    let from_str: Option<String> = opts_get_string(opts_val, "from");
    let limit: usize = match opts_get_int(opts_val, "limit") {
        Some(0) => usize::MAX,
        Some(n) if n > 0 => n as usize,
        Some(_) => {
            return (
                SIG_ERROR,
                error_val(
                    "git-error",
                    format!("{}: :limit must be non-negative", name),
                ),
            )
        }
        None => 50,
    };

    let mut walk = match repo.revwalk() {
        Ok(w) => w,
        Err(e) => return git_err(name, e),
    };

    walk.set_sorting(git2::Sort::TIME | git2::Sort::TOPOLOGICAL)
        .ok();

    let push_result = match from_str {
        Some(ref s) => match repo.revparse_single(s) {
            Ok(obj) => walk.push(obj.id()),
            Err(e) => return git_err(name, e),
        },
        None => walk.push_head(),
    };
    match push_result {
        Ok(()) => {}
        Err(e) => return git_err(name, e),
    }

    let mut result = Vec::new();
    for (i, oid_result) in walk.enumerate() {
        if i >= limit {
            break;
        }
        let oid = match oid_result {
            Ok(o) => o,
            Err(e) => return git_err(name, e),
        };
        let commit = match repo.find_commit(oid) {
            Ok(c) => c,
            Err(e) => return git_err(name, e),
        };
        result.push(commit_to_value(&commit));
    }
    (SIG_OK, elle::list(result))
}

pub fn prim_git_commit(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/commit";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let message = match get_string(args, 1, name) {
        Ok(s) => s,
        Err(e) => return e,
    };

    // Obtain index and write tree
    let mut index = match repo.index() {
        Ok(i) => i,
        Err(e) => return git_err(name, e),
    };
    let tree_oid = match index.write_tree() {
        Ok(o) => o,
        Err(e) => return git_err(name, e),
    };
    let tree = match repo.find_tree(tree_oid) {
        Ok(t) => t,
        Err(e) => return git_err(name, e),
    };

    // Get HEAD commit as parent (None for initial commit)
    let parent_commit: Option<git2::Commit<'_>> = match repo.head() {
        Ok(head) => head.peel_to_commit().ok(),
        Err(_) => None,
    };
    let parents: Vec<&git2::Commit<'_>> = parent_commit.iter().collect();

    // Extract author/committer from opts or repo config
    let opts_val = if args.len() >= 3 { Some(args[2]) } else { None };

    let author_sub = opts_get(opts_val, "author");
    let author_name = opts_get_string(author_sub, "name");
    let author_email = opts_get_string(author_sub, "email");

    let committer_sub = opts_get(opts_val, "committer");
    let committer_name = opts_get_string(committer_sub, "name");
    let committer_email = opts_get_string(committer_sub, "email");

    let author = match make_signature(repo, name, author_name, author_email) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let committer = match make_signature(repo, name, committer_name, committer_email) {
        Ok(s) => s,
        Err(e) => return e,
    };

    match repo.commit(Some("HEAD"), &author, &committer, &message, &tree, &parents) {
        Ok(oid) => (SIG_OK, Value::string(oid.to_string())),
        Err(e) => git_err(name, e),
    }
}
