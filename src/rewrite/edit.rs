//! Edit types and application.

/// A source edit: replace bytes at [byte_offset..byte_offset+byte_len] with replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Edit {
    pub byte_offset: usize,
    pub byte_len: usize,
    pub replacement: String,
}

/// Apply edits to source text. Sorts back-to-front so byte offsets remain valid.
/// Returns Err if any edits overlap.
pub(crate) fn apply_edits(source: &str, edits: &mut [Edit]) -> Result<String, String> {
    edits.sort_by_key(|e| std::cmp::Reverse(e.byte_offset));

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
