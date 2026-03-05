//! Edit types and application.

/// A source edit: replace bytes at [byte_offset..byte_offset+byte_len] with replacement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub byte_offset: usize,
    pub byte_len: usize,
    pub replacement: String,
}

/// Apply edits to source text. Sorts back-to-front so byte offsets remain valid.
/// Returns Err if any edits overlap.
pub fn apply_edits(source: &str, edits: &mut [Edit]) -> Result<String, String> {
    edits.sort_by(|a, b| b.byte_offset.cmp(&a.byte_offset));

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
mod tests {
    use super::*;

    #[test]
    fn test_single_edit() {
        let source = "(path/join a b)";
        let mut edits = vec![Edit {
            byte_offset: 1,
            byte_len: 9,
            replacement: "path-join".to_string(),
        }];
        assert_eq!(apply_edits(source, &mut edits).unwrap(), "(path-join a b)");
    }

    #[test]
    fn test_multiple_edits() {
        let source = "(path/join (path/parent x))";
        let mut edits = vec![
            Edit {
                byte_offset: 1,
                byte_len: 9,
                replacement: "path-join".to_string(),
            },
            Edit {
                byte_offset: 12,
                byte_len: 11,
                replacement: "path-parent".to_string(),
            },
        ];
        assert_eq!(
            apply_edits(source, &mut edits).unwrap(),
            "(path-join (path-parent x))"
        );
    }

    #[test]
    fn test_empty_edits() {
        let source = "(+ 1 2)";
        let mut edits: Vec<Edit> = vec![];
        assert_eq!(apply_edits(source, &mut edits).unwrap(), "(+ 1 2)");
    }

    #[test]
    fn test_different_length_replacement() {
        let source = "(fn/arity f)";
        let mut edits = vec![Edit {
            byte_offset: 1,
            byte_len: 8,
            replacement: "fn-arity".to_string(),
        }];
        assert_eq!(apply_edits(source, &mut edits).unwrap(), "(fn-arity f)");
    }

    #[test]
    fn test_overlapping_edits_error() {
        let source = "abcdefgh";
        let mut edits = vec![
            Edit {
                byte_offset: 2,
                byte_len: 4,
                replacement: "X".to_string(),
            },
            Edit {
                byte_offset: 4,
                byte_len: 3,
                replacement: "Y".to_string(),
            },
        ];
        let result = apply_edits(source, &mut edits);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("overlapping edits"));
    }
}
