// audited: 2026-11-20
//! A byte-span edit to source text, and the one function that applies a set of them.
//!
//! docs/epochs.md

/// A source edit: replace bytes at [byte_offset..byte_offset+byte_len] with replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Edit {
    pub byte_offset: usize,
    pub byte_len: usize,
    pub replacement: String,
}

impl Edit {
    /// The edit that writes `replacement` over the bytes from `from` up to `to`.
    pub(crate) fn replace(from: usize, to: usize, replacement: &str) -> Edit {
        Edit {
            byte_offset: from,
            byte_len: to - from,
            replacement: replacement.to_string(),
        }
    }

    /// The edit that writes `text` at `at`.
    pub(crate) fn insert(at: usize, text: &str) -> Edit {
        Edit::replace(at, at, text)
    }
}

/// Apply edits to source text. Sorts back-to-front so byte offsets remain valid.
/// Returns Err if any edits overlap.
///
/// An insertion and a replacement may start at one offset — a wrap opening
/// where a renamed symbol stands. The replacement applies first, so the
/// insertion lands before its new text rather than inside it.
pub(crate) fn apply_edits(source: &str, edits: &mut [Edit]) -> Result<String, String> {
    edits.sort_by_key(|e| std::cmp::Reverse((e.byte_offset, e.byte_len)));

    // Check for overlaps (edits are now sorted descending by offset)
    for window in edits.windows(2) {
        let later = &window[0];
        let earlier = &window[1];
        let earlier_end = earlier.byte_offset + earlier.byte_len;
        if earlier_end > later.byte_offset {
            return Err(format!(
                "overlapping edits: [{}, {}) and [{}, {})",
                earlier.byte_offset,
                earlier_end,
                later.byte_offset,
                later.byte_offset + later.byte_len,
            ));
        }
    }

    let mut result = source.to_string();
    for edit in edits.iter() {
        result.replace_range(
            edit.byte_offset..edit.byte_offset + edit.byte_len,
            &edit.replacement,
        );
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
