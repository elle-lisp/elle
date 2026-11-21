// audited: 2026-11-20
//! FileOp — a metadata operation or a sync on a file named by its path, and the kind of answer its completion builds.
//!
//! docs/impl/io/platforms.md
//! docs/io/files.md

/// An operation on a file named by its path, which the ring or a pool worker
/// runs (docs/impl/io/platforms.md). None moves a file's contents; the calls
/// that do are port calls (docs/io/files.md).
///
/// Every field is owned and `Send`, so the submission clones the operation
/// into the `PoolOp` the worker runs.
#[derive(Debug, Clone)]
pub enum FileOp {
    /// Flush the file or directory at `path` to stable storage, as fsync(2)
    /// does. Returns nil.
    Sync { path: String },
    /// Read the metadata of what `path` names, through a final symlink when
    /// `follow` holds. Returns the stat struct.
    Stat { path: String, follow: bool },
    /// Read the size of the file `path` names. Returns an int.
    Size { path: String },
    /// Read the names in the directory `path` names. Returns a list of strings.
    List { path: String },
    /// Make the directory `path`, and every missing parent when `parents`
    /// holds. Returns true.
    Mkdir { path: String, parents: bool },
    /// Remove the file `path` names. Returns true.
    Delete { path: String },
    /// Remove the directory `path`, and everything under it when `recursive`
    /// holds. Returns true.
    DeleteDir { path: String, recursive: bool },
    /// Rename `from` to `to`. Returns true.
    Rename { from: String, to: String },
    /// Make a directory with a name nothing else holds, under `root`.
    /// Returns its path.
    MakeTempDir { root: String },
    /// Ask whether `path` names what `test` asks for. Returns a bool, and
    /// false for a path it cannot reach.
    Probe { path: String, test: PathTest },
    /// Resolve `path` to an absolute one with no symlink in it. Returns the
    /// resolved path.
    Canonicalize { path: String },
}

/// What a [`FileOp::Probe`] asks of a path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathTest {
    /// Anything is there, through a final symlink.
    Exists,
    /// A regular file is there.
    File,
    /// A directory is there.
    Dir,
}

/// What a completion builds from a [`FileOp`]'s result, which the pending
/// entry keeps in place of the operation.
///
/// A kind carries what picks the primitive its error names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Sync,
    Stat { follow: bool },
    Size,
    List,
    Mkdir { parents: bool },
    Delete,
    DeleteDir { recursive: bool },
    Rename,
    MakeTempDir,
    Probe(PathTest),
    Canonicalize,
}

impl FileKind {
    /// The primitive that raises this kind of operation, which its error names.
    pub fn primitive(self) -> &'static str {
        match self {
            FileKind::Sync => "file/sync",
            FileKind::Stat { follow: true } => "file/stat",
            FileKind::Stat { follow: false } => "file/lstat",
            FileKind::Size => "file/size",
            FileKind::List => "file/ls",
            FileKind::Mkdir { parents: false } => "file/mkdir",
            FileKind::Mkdir { parents: true } => "file/mkdir-all",
            FileKind::Delete => "file/delete",
            FileKind::DeleteDir { recursive: false } => "file/delete-dir",
            FileKind::DeleteDir { recursive: true } => "file/delete-dir-all",
            FileKind::Rename => "file/rename",
            FileKind::MakeTempDir => "file/mktempdir",
            FileKind::Probe(PathTest::Exists) => "path/exists?",
            FileKind::Probe(PathTest::File) => "path/file?",
            FileKind::Probe(PathTest::Dir) => "path/dir?",
            FileKind::Canonicalize => "path/canonicalize",
        }
    }
}

impl FileOp {
    /// The kind of answer this operation's completion builds.
    pub fn kind(&self) -> FileKind {
        match self {
            FileOp::Sync { .. } => FileKind::Sync,
            FileOp::Stat { follow, .. } => FileKind::Stat { follow: *follow },
            FileOp::Size { .. } => FileKind::Size,
            FileOp::List { .. } => FileKind::List,
            FileOp::Mkdir { parents, .. } => FileKind::Mkdir { parents: *parents },
            FileOp::Delete { .. } => FileKind::Delete,
            FileOp::DeleteDir { recursive, .. } => FileKind::DeleteDir {
                recursive: *recursive,
            },
            FileOp::Rename { .. } => FileKind::Rename,
            FileOp::MakeTempDir { .. } => FileKind::MakeTempDir,
            FileOp::Probe { test, .. } => FileKind::Probe(*test),
            FileOp::Canonicalize { .. } => FileKind::Canonicalize,
        }
    }

    /// The path the operation names, which its error reports: for a rename,
    /// the file it moves from, and for a temporary directory, the root it
    /// makes one under.
    pub fn path(&self) -> &str {
        match self {
            FileOp::Sync { path }
            | FileOp::Stat { path, .. }
            | FileOp::Size { path }
            | FileOp::List { path }
            | FileOp::Mkdir { path, .. }
            | FileOp::Delete { path }
            | FileOp::DeleteDir { path, .. }
            | FileOp::Probe { path, .. }
            | FileOp::Canonicalize { path } => path,
            FileOp::Rename { from, .. } => from,
            FileOp::MakeTempDir { root } => root,
        }
    }

    /// What a submission to the pool declares about the wait this operation may
    /// enter (docs/impl/io/cancel.md). A sync opens its path non-blocking, and
    /// `fsync(2)` waits on nothing outside this process. A walk takes one
    /// blocking call per entry or path component, and a stop pipe from `hub`
    /// that it checks before each. Every other operation is one blocking call
    /// that no stop pipe reaches.
    pub(in crate::io) fn bounds(
        &self,
        hub: &mut crate::io::threadpool::CompletionHub,
        id: crate::io::SubmissionId,
    ) -> crate::io::threadpool::Bounds {
        use crate::io::threadpool::Bounds;
        match self {
            FileOp::Sync { .. } => Bounds::prompt(),
            FileOp::List { .. }
            | FileOp::Mkdir { parents: true, .. }
            | FileOp::DeleteDir {
                recursive: true, ..
            }
            | FileOp::Canonicalize { .. } => hub.bounds(id, super::Bound::NONE),
            FileOp::Stat { .. }
            | FileOp::Size { .. }
            | FileOp::Mkdir { parents: false, .. }
            | FileOp::Delete { .. }
            | FileOp::DeleteDir {
                recursive: false, ..
            }
            | FileOp::Rename { .. }
            | FileOp::MakeTempDir { .. }
            | FileOp::Probe { .. } => Bounds::uninterruptible(),
        }
    }
}
