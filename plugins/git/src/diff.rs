//! Diff primitives.

use crate::helpers::{diff_to_value, get_diff, get_repo, git_err};
use elle::value::fiber::{SignalBits, SIG_OK};
use elle::value::Value;

pub fn prim_git_diff(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/diff";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let opts_val = if args.len() >= 2 { Some(args[1]) } else { None };
    let diff = match get_diff(repo, opts_val, name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    match diff_to_value(&diff) {
        Ok(v) => (SIG_OK, v),
        Err(e) => git_err(name, e),
    }
}

pub fn prim_git_diff_patch(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/diff-patch";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let opts_val = if args.len() >= 2 { Some(args[1]) } else { None };
    let diff = match get_diff(repo, opts_val, name) {
        Ok(d) => d,
        Err(e) => return e,
    };
    let mut patch_text = String::new();
    let result = diff.print(git2::DiffFormat::Patch, |_delta, _hunk, line| {
        let content = std::str::from_utf8(line.content()).unwrap_or("");
        match line.origin() {
            '+' | '-' | ' ' => patch_text.push(line.origin()),
            _ => {}
        }
        patch_text.push_str(content);
        true
    });
    match result {
        Ok(()) => (SIG_OK, Value::string(patch_text)),
        Err(e) => git_err(name, e),
    }
}

pub fn prim_git_show(args: &[Value]) -> (SignalBits, Value) {
    use crate::helpers::{get_string, git_err};
    use elle::value::error_val;
    use elle::value::fiber::SIG_ERROR;

    let name = "git/show";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let refname = match get_string(args, 1, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let path = match get_string(args, 2, name) {
        Ok(s) => s,
        Err(e) => return e,
    };

    let spec = format!("{}:{}", refname, path);
    let obj = match repo.revparse_single(&spec) {
        Ok(o) => o,
        Err(e) => return git_err(name, e),
    };
    let blob = match obj.peel_to_blob() {
        Ok(b) => b,
        Err(e) => return git_err(name, e),
    };
    if blob.is_binary() {
        return (
            SIG_ERROR,
            error_val("git-error", format!("{}: binary file: {}", name, path)),
        );
    }
    match std::str::from_utf8(blob.content()) {
        Ok(s) => (SIG_OK, Value::string(s.to_string())),
        Err(_) => (
            SIG_ERROR,
            error_val(
                "git-error",
                format!("{}: invalid UTF-8 in file: {}", name, path),
            ),
        ),
    }
}
