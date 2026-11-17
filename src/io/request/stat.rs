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

/// The little-endian words an encoded record takes: a head word, the three
/// times, then the size and the nine Unix fields.
const WORDS: usize = 14;

/// The head word's low byte is the file type's code, and the three bits above
/// it say which times are present. Every higher bit is zero.
const TIMES_SHIFT: u32 = 8;

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

    fn of(ft: std::fs::FileType) -> FileType {
        if ft.is_file() {
            FileType::File
        } else if ft.is_dir() {
            FileType::Dir
        } else if ft.is_symlink() {
            FileType::Symlink
        } else {
            FileType::Other
        }
    }

    fn code(self) -> u64 {
        match self {
            FileType::File => 0,
            FileType::Dir => 1,
            FileType::Symlink => 2,
            FileType::Other => 3,
        }
    }

    fn from_code(code: u64) -> Option<FileType> {
        [
            FileType::File,
            FileType::Dir,
            FileType::Symlink,
            FileType::Other,
        ]
        .into_iter()
        .find(|t| t.code() == code)
    }
}

/// Seconds since the epoch, or `None` for a time the OS does not keep or one
/// before the epoch.
fn seconds(time: std::io::Result<std::time::SystemTime>) -> Option<f64> {
    let since = time.ok()?.duration_since(std::time::UNIX_EPOCH).ok()?;
    Some(since.as_secs_f64())
}

impl StatRecord {
    /// The record of what `meta` reports.
    pub fn from_metadata(meta: &std::fs::Metadata) -> StatRecord {
        use std::os::unix::fs::MetadataExt;
        StatRecord {
            file_type: FileType::of(meta.file_type()),
            size: meta.len(),
            accessed: seconds(meta.accessed()),
            created: seconds(meta.created()),
            modified: seconds(meta.modified()),
            permissions: u64::from(meta.mode()),
            uid: u64::from(meta.uid()),
            gid: u64::from(meta.gid()),
            nlinks: meta.nlink(),
            inode: meta.ino(),
            dev: meta.dev(),
            rdev: meta.rdev(),
            blocks: meta.blocks(),
            blksize: meta.blksize(),
        }
    }

    /// Whether no write permission bit is set, as `Permissions::readonly`
    /// answers on Unix.
    pub fn readonly(&self) -> bool {
        self.permissions & 0o222 == 0
    }

    /// The record as fixed-width little-endian bytes.
    pub fn encode(&self) -> Vec<u8> {
        let times = [self.accessed, self.created, self.modified];
        let present = times
            .iter()
            .enumerate()
            .fold(0, |bits, (i, t)| bits | u64::from(t.is_some()) << i);
        let time = |t: Option<f64>| t.map_or(0, f64::to_bits);
        let words: [u64; WORDS] = [
            self.file_type.code() | present << TIMES_SHIFT,
            time(self.accessed),
            time(self.created),
            time(self.modified),
            self.size,
            self.permissions,
            self.uid,
            self.gid,
            self.nlinks,
            self.inode,
            self.dev,
            self.rdev,
            self.blocks,
            self.blksize,
        ];
        words.iter().flat_map(|w| w.to_le_bytes()).collect()
    }

    /// The record `bytes` encode, or `None` for bytes that are not exactly
    /// one encoded record.
    pub fn decode(bytes: &[u8]) -> Option<StatRecord> {
        if bytes.len() != WORDS * 8 {
            return None;
        }
        let mut w = [0u64; WORDS];
        for (word, chunk) in w.iter_mut().zip(bytes.as_chunks::<8>().0) {
            *word = u64::from_le_bytes(*chunk);
        }
        let head = w[0];
        if head >> (TIMES_SHIFT + 3) != 0 {
            return None;
        }
        let file_type = FileType::from_code(head & 0xff)?;
        let time = |i: usize| {
            (head >> (TIMES_SHIFT as usize + i) & 1 == 1).then(|| f64::from_bits(w[1 + i]))
        };
        Some(StatRecord {
            file_type,
            size: w[4],
            accessed: time(0),
            created: time(1),
            modified: time(2),
            permissions: w[5],
            uid: w[6],
            gid: w[7],
            nlinks: w[8],
            inode: w[9],
            dev: w[10],
            rdev: w[11],
            blocks: w[12],
            blksize: w[13],
        })
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
