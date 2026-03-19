//! Shared helpers for the elle-git plugin.

use elle::value::fiber::{SignalBits, SIG_ERROR};
use elle::value::{error_val, TableKey, Value};
use git2::Repository;
use std::cell::RefCell;
use std::collections::BTreeMap;

// ---------------------------------------------------------------------------
// Basic extraction helpers
// ---------------------------------------------------------------------------

/// Extract the git2::Repository from an External value or return a type-error.
pub fn get_repo<'a>(args: &'a [Value], name: &str) -> Result<&'a Repository, (SignalBits, Value)> {
    args[0].as_external::<Repository>().ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!("{}: expected git/repo, got {}", name, args[0].type_name()),
            ),
        )
    })
}

/// Extract a string from args[idx] or return a type-error.
pub fn get_string(args: &[Value], idx: usize, name: &str) -> Result<String, (SignalBits, Value)> {
    args[idx].with_string(|s| s.to_string()).ok_or_else(|| {
        (
            SIG_ERROR,
            error_val(
                "type-error",
                format!(
                    "{}: expected string at arg {}, got {}",
                    name,
                    idx,
                    args[idx].type_name()
                ),
            ),
        )
    })
}

/// Wrap a git2::Error into an error signal.
pub fn git_err(name: &str, e: git2::Error) -> (SignalBits, Value) {
    (
        SIG_ERROR,
        error_val("git-error", format!("{}: {}", name, e.message())),
    )
}

// ---------------------------------------------------------------------------
// Opts struct helpers
// ---------------------------------------------------------------------------

/// Get a field from an optional opts struct by keyword name.
pub fn opts_get(opts: Option<Value>, field: &str) -> Option<Value> {
    let v = opts?;
    let map = v.as_struct()?;
    map.get(&TableKey::Keyword(field.into())).copied()
}

/// Get a string field from an opts struct.
pub fn opts_get_string(opts: Option<Value>, field: &str) -> Option<String> {
    opts_get(opts, field).and_then(|v| v.with_string(|s| s.to_string()))
}

/// Get an integer field from an opts struct.
pub fn opts_get_int(opts: Option<Value>, field: &str) -> Option<i64> {
    opts_get(opts, field).and_then(|v| v.as_int())
}

/// Get a truthy field from an opts struct (true if field exists and is truthy).
pub fn opts_get_bool(opts: Option<Value>, field: &str) -> bool {
    opts_get(opts, field)
        .map(|v| v.is_truthy())
        .unwrap_or(false)
}

// ---------------------------------------------------------------------------
// Path extraction
// ---------------------------------------------------------------------------

/// Extract a list of paths from a string or array argument.
pub fn extract_paths(
    val: Value,
    name: &str,
) -> Result<Vec<std::path::PathBuf>, (SignalBits, Value)> {
    if let Some(s) = val.with_string(|s| s.to_string()) {
        return Ok(vec![std::path::PathBuf::from(s)]);
    }
    if let Some(arr) = val.as_array() {
        let mut paths = Vec::new();
        for item in arr.iter() {
            match item.with_string(|s| s.to_string()) {
                Some(s) => paths.push(std::path::PathBuf::from(s)),
                None => {
                    return Err((
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "{}: path array must contain strings, got {}",
                                name,
                                item.type_name()
                            ),
                        ),
                    ))
                }
            }
        }
        return Ok(paths);
    }
    if let Some(arr) = val.as_array_mut() {
        let arr = arr.borrow();
        let mut paths = Vec::new();
        for item in arr.iter() {
            match item.with_string(|s| s.to_string()) {
                Some(s) => paths.push(std::path::PathBuf::from(s)),
                None => {
                    return Err((
                        SIG_ERROR,
                        error_val(
                            "type-error",
                            format!(
                                "{}: path array must contain strings, got {}",
                                name,
                                item.type_name()
                            ),
                        ),
                    ))
                }
            }
        }
        return Ok(paths);
    }
    Err((
        SIG_ERROR,
        error_val(
            "type-error",
            format!(
                "{}: expected string or array of strings, got {}",
                name,
                val.type_name()
            ),
        ),
    ))
}

// ---------------------------------------------------------------------------
// Value conversion helpers
// ---------------------------------------------------------------------------

/// Convert a git2::RepositoryState to a keyword value.
pub fn repo_state_to_keyword(state: git2::RepositoryState) -> Value {
    use git2::RepositoryState::*;
    let kw = match state {
        Clean => "clean",
        Merge => "merge",
        Revert => "revert",
        RevertSequence => "revert-sequence",
        CherryPick => "cherry-pick",
        CherryPickSequence => "cherry-pick-sequence",
        Bisect => "bisect",
        Rebase => "rebase",
        RebaseInteractive => "rebase-interactive",
        RebaseMerge => "rebase-merge",
        ApplyMailbox => "apply-mailbox",
        ApplyMailboxOrRebase => "apply-mailbox-or-rebase",
    };
    Value::keyword(kw)
}

/// Convert a Signature to an Elle struct {:name :email :time}.
pub fn signature_to_value(sig: git2::Signature<'_>) -> Value {
    let name = sig.name().unwrap_or("").to_string();
    let email = sig.email().unwrap_or("").to_string();
    let time = sig.when().seconds();
    let mut fields = BTreeMap::new();
    fields.insert(TableKey::Keyword("name".into()), Value::string(name));
    fields.insert(TableKey::Keyword("email".into()), Value::string(email));
    fields.insert(TableKey::Keyword("time".into()), Value::int(time));
    Value::struct_from(fields)
}

/// Convert a Commit to an Elle struct.
pub fn commit_to_value(commit: &git2::Commit<'_>) -> Value {
    let oid = commit.id().to_string();
    let message_val = match commit.message() {
        Some(m) => Value::string(m.to_string()),
        None => Value::NIL,
    };
    let summary_val = match commit.summary() {
        Some(s) => Value::string(s.to_string()),
        None => Value::NIL,
    };
    let author = signature_to_value(commit.author());
    let committer = signature_to_value(commit.committer());
    let parents: Vec<Value> = commit
        .parent_ids()
        .map(|id| Value::string(id.to_string()))
        .collect();
    let tree_oid = commit.tree_id().to_string();

    let mut fields = BTreeMap::new();
    fields.insert(TableKey::Keyword("oid".into()), Value::string(oid));
    fields.insert(TableKey::Keyword("message".into()), message_val);
    fields.insert(TableKey::Keyword("summary".into()), summary_val);
    fields.insert(TableKey::Keyword("author".into()), author);
    fields.insert(TableKey::Keyword("committer".into()), committer);
    fields.insert(TableKey::Keyword("parents".into()), Value::array(parents));
    fields.insert(TableKey::Keyword("tree".into()), Value::string(tree_oid));
    Value::struct_from(fields)
}

/// Convert a branch (name, oid, kind, upstream) to an Elle struct.
pub fn branch_to_value(
    name: &str,
    oid: git2::Oid,
    kind: git2::BranchType,
    upstream: Option<String>,
) -> Value {
    let kind_kw = match kind {
        git2::BranchType::Local => Value::keyword("local"),
        git2::BranchType::Remote => Value::keyword("remote"),
    };
    let upstream_val = match upstream {
        Some(s) => Value::string(s),
        None => Value::NIL,
    };
    let mut fields = BTreeMap::new();
    fields.insert(TableKey::Keyword("name".into()), Value::string(name));
    fields.insert(
        TableKey::Keyword("oid".into()),
        Value::string(oid.to_string()),
    );
    fields.insert(TableKey::Keyword("kind".into()), kind_kw);
    fields.insert(TableKey::Keyword("upstream".into()), upstream_val);
    Value::struct_from(fields)
}

/// Map a Status bitfield to a keyword for the index or workdir column.
pub fn status_to_keyword(status: git2::Status, index: bool) -> Value {
    if index {
        if status.contains(git2::Status::INDEX_NEW) {
            return Value::keyword("new");
        }
        if status.contains(git2::Status::INDEX_MODIFIED) {
            return Value::keyword("modified");
        }
        if status.contains(git2::Status::INDEX_DELETED) {
            return Value::keyword("deleted");
        }
        if status.contains(git2::Status::INDEX_RENAMED) {
            return Value::keyword("renamed");
        }
        if status.contains(git2::Status::INDEX_TYPECHANGE) {
            return Value::keyword("typechange");
        }
        if status.contains(git2::Status::CONFLICTED) {
            return Value::keyword("conflicted");
        }
    } else {
        if status.contains(git2::Status::WT_NEW) {
            return Value::keyword("new");
        }
        if status.contains(git2::Status::WT_MODIFIED) {
            return Value::keyword("modified");
        }
        if status.contains(git2::Status::WT_DELETED) {
            return Value::keyword("deleted");
        }
        if status.contains(git2::Status::WT_RENAMED) {
            return Value::keyword("renamed");
        }
        if status.contains(git2::Status::WT_TYPECHANGE) {
            return Value::keyword("typechange");
        }
        if status.contains(git2::Status::CONFLICTED) {
            return Value::keyword("conflicted");
        }
    }
    Value::NIL
}

// ---------------------------------------------------------------------------
// Diff helpers
// ---------------------------------------------------------------------------

pub fn diff_status_to_keyword(delta: git2::Delta) -> Value {
    use git2::Delta::*;
    let kw = match delta {
        Added => "added",
        Deleted => "deleted",
        Modified => "modified",
        Renamed => "renamed",
        Copied => "copied",
        Typechange => "typechange",
        _ => "modified",
    };
    Value::keyword(kw)
}

pub fn line_origin_to_keyword(origin: char) -> Value {
    match origin {
        '+' => Value::keyword("add"),
        '-' => Value::keyword("delete"),
        ' ' => Value::keyword("context"),
        _ => Value::keyword("context"),
    }
}

pub fn build_hunk_value(
    header: String,
    old_start: u32,
    old_lines: u32,
    new_start: u32,
    new_lines: u32,
    lines: Vec<Value>,
) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(TableKey::Keyword("header".into()), Value::string(header));
    fields.insert(
        TableKey::Keyword("old-start".into()),
        Value::int(old_start as i64),
    );
    fields.insert(
        TableKey::Keyword("old-lines".into()),
        Value::int(old_lines as i64),
    );
    fields.insert(
        TableKey::Keyword("new-start".into()),
        Value::int(new_start as i64),
    );
    fields.insert(
        TableKey::Keyword("new-lines".into()),
        Value::int(new_lines as i64),
    );
    fields.insert(TableKey::Keyword("lines".into()), Value::array(lines));
    Value::struct_from(fields)
}

pub fn build_file_value(
    path: String,
    old_path: String,
    delta: git2::Delta,
    hunks: Vec<Value>,
) -> Value {
    let mut fields = BTreeMap::new();
    fields.insert(TableKey::Keyword("path".into()), Value::string(path));
    fields.insert(
        TableKey::Keyword("old-path".into()),
        Value::string(old_path),
    );
    fields.insert(
        TableKey::Keyword("status".into()),
        diff_status_to_keyword(delta),
    );
    fields.insert(TableKey::Keyword("hunks".into()), Value::array(hunks));
    Value::struct_from(fields)
}

/// Mutable state accumulator for the diff foreach callbacks.
pub struct DiffState {
    pub files: Vec<Value>,
    pub cur_path: String,
    pub cur_old_path: String,
    pub cur_delta: git2::Delta,
    pub cur_hunks: Vec<Value>,
    pub cur_hunk_header: Option<(String, u32, u32, u32, u32)>,
    pub cur_lines: Vec<Value>,
    pub in_file: bool,
}

impl DiffState {
    pub fn new() -> Self {
        DiffState {
            files: Vec::new(),
            cur_path: String::new(),
            cur_old_path: String::new(),
            cur_delta: git2::Delta::Unmodified,
            cur_hunks: Vec::new(),
            cur_hunk_header: None,
            cur_lines: Vec::new(),
            in_file: false,
        }
    }

    pub fn finalize_hunk(&mut self) {
        if let Some((hdr, os, ol, ns, nl)) = self.cur_hunk_header.take() {
            let hunk_val =
                build_hunk_value(hdr, os, ol, ns, nl, std::mem::take(&mut self.cur_lines));
            self.cur_hunks.push(hunk_val);
        }
    }

    pub fn finalize_file(&mut self) {
        if self.in_file {
            self.finalize_hunk();
            let hunks = std::mem::take(&mut self.cur_hunks);
            let file_val = build_file_value(
                self.cur_path.clone(),
                self.cur_old_path.clone(),
                self.cur_delta,
                hunks,
            );
            self.files.push(file_val);
            self.in_file = false;
        }
    }
}

/// Build a diff value from a git2::Diff.
pub fn diff_to_value(diff: &git2::Diff<'_>) -> Result<Value, git2::Error> {
    let stats = diff.stats()?;
    let state = RefCell::new(DiffState::new());

    let mut file_cb = |delta: git2::DiffDelta<'_>, _progress: f32| -> bool {
        let mut s = state.borrow_mut();
        s.finalize_file();
        let new_file = delta.new_file();
        let old_file = delta.old_file();
        s.cur_path = new_file
            .path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();
        s.cur_old_path = old_file
            .path()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_else(|| s.cur_path.clone());
        s.cur_delta = delta.status();
        s.in_file = true;
        true
    };
    let mut hunk_cb = |_delta: git2::DiffDelta<'_>, hunk: git2::DiffHunk<'_>| -> bool {
        let mut s = state.borrow_mut();
        s.finalize_hunk();
        let header = std::str::from_utf8(hunk.header())
            .unwrap_or("")
            .trim_end()
            .to_string();
        s.cur_hunk_header = Some((
            header,
            hunk.old_start(),
            hunk.old_lines(),
            hunk.new_start(),
            hunk.new_lines(),
        ));
        true
    };
    let mut line_cb = |_delta: git2::DiffDelta<'_>,
                       _hunk: Option<git2::DiffHunk<'_>>,
                       line: git2::DiffLine<'_>|
     -> bool {
        let mut s = state.borrow_mut();
        let origin = line.origin();
        let content = std::str::from_utf8(line.content())
            .unwrap_or("")
            .to_string();
        let mut fields = BTreeMap::new();
        fields.insert(
            TableKey::Keyword("origin".into()),
            line_origin_to_keyword(origin),
        );
        fields.insert(TableKey::Keyword("content".into()), Value::string(content));
        s.cur_lines.push(Value::struct_from(fields));
        true
    };
    diff.foreach(&mut file_cb, None, Some(&mut hunk_cb), Some(&mut line_cb))?;

    // Finalize last file
    state.borrow_mut().finalize_file();
    let s = state.into_inner();

    let mut result = BTreeMap::new();
    result.insert(
        TableKey::Keyword("files-changed".into()),
        Value::int(stats.files_changed() as i64),
    );
    result.insert(
        TableKey::Keyword("insertions".into()),
        Value::int(stats.insertions() as i64),
    );
    result.insert(
        TableKey::Keyword("deletions".into()),
        Value::int(stats.deletions() as i64),
    );
    result.insert(TableKey::Keyword("files".into()), Value::array(s.files));
    Ok(Value::struct_from(result))
}

/// Get the diff for a repo given opts (cached or workdir).
pub fn get_diff<'repo>(
    repo: &'repo Repository,
    opts_val: Option<Value>,
    name: &str,
) -> Result<git2::Diff<'repo>, (SignalBits, Value)> {
    let cached = opts_get_bool(opts_val, "cached");

    if cached {
        let head = repo.head().map_err(|e| git_err(name, e))?;
        let head_commit = head.peel_to_commit().map_err(|e| git_err(name, e))?;
        let head_tree = head_commit.tree().map_err(|e| git_err(name, e))?;
        let index = repo.index().map_err(|e| git_err(name, e))?;
        repo.diff_tree_to_index(Some(&head_tree), Some(&index), None)
            .map_err(|e| git_err(name, e))
    } else {
        repo.diff_index_to_workdir(None, None)
            .map_err(|e| git_err(name, e))
    }
}

/// Build a git2::Signature for now using repo config or provided name/email.
pub fn make_signature(
    repo: &Repository,
    name: &str,
    sig_name: Option<String>,
    sig_email: Option<String>,
) -> Result<git2::Signature<'static>, (SignalBits, Value)> {
    let config = repo.config().map_err(|e| git_err(name, e))?;

    let resolved_name = match sig_name {
        Some(n) => n,
        None => match config.get_string("user.name") {
            Ok(n) => n,
            Err(e) => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "git-error",
                        format!("{}: user.name not configured: {}", name, e.message()),
                    ),
                ))
            }
        },
    };

    let resolved_email = match sig_email {
        Some(e) => e,
        None => match config.get_string("user.email") {
            Ok(e) => e,
            Err(e) => {
                return Err((
                    SIG_ERROR,
                    error_val(
                        "git-error",
                        format!("{}: user.email not configured: {}", name, e.message()),
                    ),
                ))
            }
        },
    };

    let now = git2::Time::new(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64,
        0,
    );

    git2::Signature::new(&resolved_name, &resolved_email, &now).map_err(|e| git_err(name, e))
}
