//! Tag primitives.

use crate::helpers::{get_repo, get_string, git_err, make_signature};
use elle::value::fiber::{SignalBits, SIG_OK};
use elle::value::Value;

pub fn prim_git_tags(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/tags";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let mut tag_names: Vec<Value> = Vec::new();
    match repo.tag_names(None) {
        Ok(names) => {
            for name_opt in names.iter() {
                if let Some(n) = name_opt {
                    tag_names.push(Value::string(n.to_string()));
                }
            }
        }
        Err(e) => return git_err(name, e),
    }
    (SIG_OK, elle::list(tag_names))
}

pub fn prim_git_tag_create(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/tag-create";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let tag_name = match get_string(args, 1, name) {
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

    if args.len() >= 4 {
        // Annotated tag
        let message = match get_string(args, 3, name) {
            Ok(s) => s,
            Err(e) => return e,
        };
        let tagger = match make_signature(repo, name, None, None) {
            Ok(s) => s,
            Err(e) => return e,
        };
        match repo.tag(&tag_name, &target_obj, &tagger, &message, false) {
            Ok(oid) => (SIG_OK, Value::string(oid.to_string())),
            Err(e) => git_err(name, e),
        }
    } else {
        // Lightweight tag
        match repo.tag_lightweight(&tag_name, &target_obj, false) {
            Ok(oid) => (SIG_OK, Value::string(oid.to_string())),
            Err(e) => git_err(name, e),
        }
    }
}

pub fn prim_git_tag_delete(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/tag-delete";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let tag_name = match get_string(args, 1, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    match repo.tag_delete(&tag_name) {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(e) => git_err(name, e),
    }
}
