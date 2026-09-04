//! The source-file interner behind [`Span`](super::Span)'s `file` field.
//!
//! A span used to carry `Option<String>`: one Rust-heap allocation per node,
//! cloned on every span merge and every syntax copy. A [`Span`](super::Span)
//! is region-resident POD now (docs/impl/syntax.md § "Span"), so the name
//! became a `FileId` — a `u32` index into this table.
//!
//! The table is process-wide and append-only. Its domain is source file names,
//! so it is bounded by the files a process reads, and an entry lives as long as
//! the process that learned it. That is what lets [`name`] hand back a
//! `&'static str`, which in turn keeps every reader of a file name — error
//! formatting, `meta/origin`, the LSP — free of a lifetime and a lock.

use std::sync::{LazyLock, RwLock};

use rustc_hash::FxHashMap;

/// A source file name, interned. `FileId::NONE` is the absent name: a
/// synthetic span, or a span the reader built from an unknown-origin token.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, PartialOrd, Ord)]
pub struct FileId(u32);

impl FileId {
    /// The absent file name. Interning starts at 1, so index 0 is free to
    /// mean "no file" and `Default` agrees with it.
    pub const NONE: FileId = FileId(0);

    /// Does this id name a file?
    pub fn is_some(self) -> bool {
        self != FileId::NONE
    }
}

/// Names in id order (index 0 is the absent name, so `names[0]` is unused),
/// plus the reverse map that keeps interning idempotent.
struct Files {
    names: Vec<&'static str>,
    ids: FxHashMap<&'static str, FileId>,
}

static FILES: LazyLock<RwLock<Files>> = LazyLock::new(|| {
    RwLock::new(Files {
        names: vec![""],
        ids: FxHashMap::default(),
    })
});

/// The id for `name`, minting one if this process has not seen it.
///
/// An empty name interns to [`FileId::NONE`], so a caller that has an empty
/// string where a file name belongs gets the same span as one that has none.
pub fn intern(name: &str) -> FileId {
    if name.is_empty() {
        return FileId::NONE;
    }
    if let Some(id) = FILES.read().unwrap().ids.get(name) {
        return *id;
    }
    let mut files = FILES.write().unwrap();
    // Re-check under the write lock: two threads reading the same new file
    // race here, and the loser must return the winner's id, not mint a second.
    if let Some(id) = files.ids.get(name) {
        return *id;
    }
    let leaked: &'static str = Box::leak(name.to_string().into_boxed_str());
    let id = FileId(files.names.len() as u32);
    files.names.push(leaked);
    files.ids.insert(leaked, id);
    id
}

/// The name `id` was interned under, or `None` for [`FileId::NONE`].
pub fn name(id: FileId) -> Option<&'static str> {
    if id == FileId::NONE {
        return None;
    }
    FILES.read().unwrap().names.get(id.0 as usize).copied()
}

#[cfg(test)]
mod tests;
