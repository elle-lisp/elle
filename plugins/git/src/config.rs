//! Config primitives.

use crate::helpers::{get_repo, get_string, git_err};
use elle::value::fiber::{SignalBits, SIG_OK};
use elle::value::Value;

pub fn prim_git_config_get(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/config-get";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let key = match get_string(args, 1, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let config = match repo.config() {
        Ok(c) => c,
        Err(e) => return git_err(name, e),
    };
    match config.get_string(&key) {
        Ok(val) => (SIG_OK, Value::string(val)),
        Err(e) if e.code() == git2::ErrorCode::NotFound => (SIG_OK, Value::NIL),
        Err(e) => git_err(name, e),
    }
}

pub fn prim_git_config_set(args: &[Value]) -> (SignalBits, Value) {
    let name = "git/config-set";
    let repo = match get_repo(args, name) {
        Ok(r) => r,
        Err(e) => return e,
    };
    let key = match get_string(args, 1, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let val = match get_string(args, 2, name) {
        Ok(s) => s,
        Err(e) => return e,
    };
    let mut config = match repo.config() {
        Ok(c) => c,
        Err(e) => return git_err(name, e),
    };
    match config.set_str(&key, &val) {
        Ok(()) => (SIG_OK, Value::NIL),
        Err(e) => git_err(name, e),
    }
}
