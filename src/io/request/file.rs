// audited: 2026-10-21
//! FileOp — an operation on a file named by its path, and the kind of answer its completion builds.
//!
//! src/io/AGENTS.md

/// An operation on a file named by its path, which a pool worker runs on every
/// platform (src/io/AGENTS.md).
///
/// Every field is owned and `Send`, so the submission clones the operation
/// into the `PoolOp` the worker runs.
#[derive(Debug, Clone)]
pub enum FileOp {
    /// Read the whole file. Returns its text.
    Read { path: String },
    /// Read the whole file. Returns a list of its lines.
    Lines { path: String },
    /// Replace the file's contents with `data`, creating it. Returns true.
    Write { path: String, data: Vec<u8> },
    /// Add `data` to the end of the file, creating it. Returns true.
    Append { path: String, data: Vec<u8> },
    /// Copy the file at `from` to `to`. Returns true.
    Copy { from: String, to: String },
    /// Flush the file or directory at `path` to stable storage, as fsync(2)
    /// does. Returns nil.
    Sync { path: String },
}

/// What a completion builds from a [`FileOp`]'s result, which the pending
/// entry keeps in place of the operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Read,
    Lines,
    Write,
    Append,
    Copy,
    Sync,
}

impl FileKind {
    /// The primitive that raises this kind of operation, which its error names.
    pub fn primitive(self) -> &'static str {
        match self {
            FileKind::Read => "file/read",
            FileKind::Lines => "file/lines",
            FileKind::Write => "file/write",
            FileKind::Append => "file/append",
            FileKind::Copy => "file/copy",
            FileKind::Sync => "file/sync",
        }
    }
}

impl FileOp {
    /// The kind of answer this operation's completion builds.
    pub fn kind(&self) -> FileKind {
        match self {
            FileOp::Read { .. } => FileKind::Read,
            FileOp::Lines { .. } => FileKind::Lines,
            FileOp::Write { .. } => FileKind::Write,
            FileOp::Append { .. } => FileKind::Append,
            FileOp::Copy { .. } => FileKind::Copy,
            FileOp::Sync { .. } => FileKind::Sync,
        }
    }

    /// The path the operation names, which its error reports: for a copy,
    /// the file it copies.
    pub fn path(&self) -> &str {
        match self {
            FileOp::Read { path }
            | FileOp::Lines { path }
            | FileOp::Write { path, .. }
            | FileOp::Append { path, .. }
            | FileOp::Sync { path } => path,
            FileOp::Copy { from, .. } => from,
        }
    }
}
