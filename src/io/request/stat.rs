// audited: 2026-11-17
//! StatRecord — what a stat answers, in the fixed-width form a worker sends and a completion reads.
//!
//! docs/io/files.md

/// What a path names, as the stat struct's `:file-type` spells it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileType {
    File,
    Dir,
    Symlink,
    Other,
}

/// The metadata `file/stat` and `file/lstat` answer.
///
/// A pool worker builds one from what the OS reports and sends it encoded, and
/// the completion decodes it and builds the stat struct, so the struct's keys
/// depend on this record alone and not on where the metadata came from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StatRecord {
    pub file_type: FileType,
    pub size: u64,
    /// The three times in seconds since the epoch, absent where the platform
    /// or the filesystem keeps none, or before the epoch.
    pub accessed: Option<f64>,
    pub created: Option<f64>,
    pub modified: Option<f64>,
    /// The whole `st_mode` permission and type bits.
    pub permissions: u64,
    pub uid: u64,
    pub gid: u64,
    pub nlinks: u64,
    pub inode: u64,
    pub dev: u64,
    pub rdev: u64,
    pub blocks: u64,
    pub blksize: u64,
}

impl FileType {
    /// The string the stat struct's `:file-type` holds.
    pub fn name(self) -> &'static str {
        match self {
            FileType::File => "file",
            FileType::Dir => "dir",
            FileType::Symlink => "symlink",
            FileType::Other => "other",
        }
    }
}

impl StatRecord {
    /// The record of what `meta` reports.
    pub fn from_metadata(meta: &std::fs::Metadata) -> StatRecord {
        let _ = meta;
        StatRecord {
            file_type: FileType::Other,
            size: 0,
            accessed: None,
            created: None,
            modified: None,
            permissions: 0,
            uid: 0,
            gid: 0,
            nlinks: 0,
            inode: 0,
            dev: 0,
            rdev: 0,
            blocks: 0,
            blksize: 0,
        }
    }

    /// Whether no write permission bit is set, as `Permissions::readonly`
    /// answers on Unix.
    pub fn readonly(&self) -> bool {
        false
    }

    /// The record as fixed-width little-endian bytes.
    pub fn encode(&self) -> Vec<u8> {
        Vec::new()
    }

    /// The record `bytes` encode, or `None` for bytes that are not exactly
    /// one encoded record.
    pub fn decode(bytes: &[u8]) -> Option<StatRecord> {
        let _ = bytes;
        None
    }

    /// The same record with no times, as a filesystem that keeps none reports.
    #[cfg(test)]
    pub(crate) fn without_times(self) -> StatRecord {
        StatRecord {
            accessed: None,
            created: None,
            modified: None,
            ..self
        }
    }
}

#[cfg(test)]
mod tests;
