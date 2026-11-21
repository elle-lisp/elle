// audited: 2026-11-20
//! The notes for each read in an included file that the epoch 18 rule leaves, at the include in the rewritten text.
//!
//! docs/epochs/bodies.md
//!
//! The compile reads an included file under the capabilities of the fiber it
//! runs for, so the tree migration reads none. `elle rewrite` runs as its
//! user, and reads each file here for the notes alone. A file it cannot find,
//! read or parse makes no note, because the compile reports it.

use super::defs::Note;
use super::*;
use crate::epoch::forward::{self, IncludedRead};
use crate::reader::read_syntax_all;
use crate::syntax::SyntaxHeap;

/// A note at each include in `text`, the rewritten file `name`, whose file
/// reads a name the include's body defines after it.
pub(crate) fn included_notes(text: &str, name: &str) -> Result<Vec<Note>, String> {
    let mut home = SyntaxHeap::new();
    let forms = read_syntax_all(home.arena(), text, name)?;
    // The included files' forms live until the reads are copied out.
    let mut files = SyntaxHeap::new();
    let arena = files.arena();
    let reads = forward::included_reads(&forms, &mut |include| {
        let path = include.resolve()?;
        let source = std::fs::read_to_string(&path).ok()?;
        let forms = read_syntax_all(arena, &source, &path).ok()?;
        Some((path, forms))
    });
    // The reader's spans start after a shebang line, and a note indexes the
    // text whole.
    let shebang = shebang_len(text);
    Ok(reads.iter().map(|read| note(read, shebang)).collect())
}

fn note(read: &IncludedRead, shebang: usize) -> Note {
    Note {
        at: read.include.start as usize + shebang,
        text: format!(
            "'{}' is read at {}, in the file this include splices, before its def after the include, and the epoch 18 rule leaves the read",
            read.name, read.at
        ),
    }
}
