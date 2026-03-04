// Property tests for string operations.
//
// Tests input-dependent behavior: boundary checking, unicode handling,
// roundtrips, and error paths. Law-based tests (identity, associativity,
// idempotence) have been migrated to Elle test scripts.

use crate::common::eval_reuse_bare as eval_source;
use elle::Value;
use proptest::prelude::*;

/// Strategy for strings that include unicode characters.
fn arb_unicode_string() -> BoxedStrategy<String> {
    prop_oneof![
        // ASCII
        5 => "[a-zA-Z0-9 ]{0,20}",
        // Latin-1 supplement (accented characters)
        2 => prop::string::string_regex("[a-zàáâãäåæçèéêë]{0,10}").unwrap(),
        // CJK
        1 => prop::string::string_regex("[一二三四五六七八九十]{0,5}").unwrap(),
        // Emoji and special cases
        1 => Just("hello".to_string()),
        1 => Just("café".to_string()),
        1 => Just("naïve".to_string()),
        1 => Just("日本語".to_string()),
        1 => Just("".to_string()),
    ]
    .boxed()
}

/// Escape a string for use in Elle source code.
/// Handles backslash and double-quote.
fn escape_for_elle(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

proptest! {
    #![proptest_config(crate::common::proptest_cases(200))]

    // =========================================================================
    // Slice/substring boundary checking
    // =========================================================================

    #[test]
    fn slice_start_end_order(s in "[a-zA-Z0-9]{2,20}", start in 0usize..10, len in 1usize..5) {
        let s_len = s.len();
        let start = start.min(s_len);
        let end = (start + len).min(s_len);
        if start <= end {
            let code = format!("(string/slice \"{}\" {} {})", s, start, end);
            let result = eval_source(&code);
            // Should succeed for valid ranges
            if let Ok(val) = result {
                let sliced = val.with_string(|s| s.to_string()).unwrap();
                // Verify it's a substring of the original
                prop_assert!(s.contains(&*sliced),
                    "slice result not substring of original");
            }
        }
    }

    #[test]
    fn slice_oob_end_returns_nil(s in "[a-zA-Z0-9]{0,20}", overshoot in 1usize..100) {
        let len = s.chars().count();
        let end = len + overshoot;
        let escaped = escape_for_elle(&s);
        let code = format!("(string/slice \"{}\" 0 {})", escaped, end);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result, Value::NIL,
            "OOB end index should return nil for {:?} with end={}", s, end);
    }

    #[test]
    fn slice_oob_start_returns_nil(s in "[a-zA-Z0-9]{0,20}", overshoot in 1usize..100) {
        let len = s.chars().count();
        let start = len + overshoot;
        let end = start + 1;
        let escaped = escape_for_elle(&s);
        let code = format!("(string/slice \"{}\" {} {})", escaped, start, end);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result, Value::NIL,
            "OOB start index should return nil for {:?} with start={}", s, start);
    }

    #[test]
    fn slice_reversed_range_returns_nil(s in "[a-zA-Z0-9]{2,20}", start in 1usize..20, gap in 1usize..10) {
        let len = s.chars().count();
        let start = start.min(len);
        let end = start.saturating_sub(gap);
        if start > end {
            let escaped = escape_for_elle(&s);
            let code = format!("(string/slice \"{}\" {} {})", escaped, start, end);
            let result = eval_source(&code).unwrap();
            prop_assert_eq!(result, Value::NIL,
                "reversed range should return nil for {:?} with start={} end={}", s, start, end);
        }
    }

    #[test]
    fn slice_empty_string_oob_returns_nil(end in 1usize..100) {
        let code = format!("(string/slice \"\" 0 {})", end);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result, Value::NIL,
            "empty string OOB should return nil with end={}", end);
    }

    // =========================================================================
    // Split / Join roundtrip
    // =========================================================================

    #[test]
    fn split_join_roundtrip(
        parts in prop::collection::vec("[a-z]{1,5}", 1..=5),
        sep in "[,;|]{1,1}",
    ) {
        let joined = parts.join(&sep);
        let code = format!(
            "(string/join (string/split \"{}\" \"{}\") \"{}\")",
            joined, sep, sep
        );
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result.with_string(|s| s.to_string()).unwrap(), joined,
            "split/join roundtrip failed for {:?} with sep {:?}", parts, sep);
    }

    #[test]
    fn split_produces_list(s in "[a-z,]{1,20}", sep in "[,;]{1,1}") {
        let code = format!("(string/split \"{}\" \"{}\")", s, sep);
        let result = eval_source(&code).unwrap();
        // Result should be a list (cons cell or empty list)
        prop_assert!(result.as_cons().is_some() || result == Value::EMPTY_LIST,
            "split should produce a list");
    }

    // =========================================================================
    // Conversion roundtrips
    // =========================================================================

    #[test]
    fn number_to_string_roundtrip(n in -10000i64..10000) {
        let code = format!("(string->integer (number->string {}))", n);
        if let Ok(result) = eval_source(&code) {
            prop_assert_eq!(result, Value::int(n),
                "number->string->integer roundtrip failed for {}", n);
        }
    }

    #[test]
    fn string_to_integer_roundtrip(n in -10000i64..10000) {
        let code = format!("(string->integer \"{}\")", n);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result, Value::int(n),
            "string->integer failed for \"{}\"", n);
    }

    #[test]
    fn string_to_integer_invalid_returns_error(s in "[a-z]{1,10}") {
        let code = format!("(string->integer \"{}\")", s);
        let result = eval_source(&code);
        // Should error on non-numeric string
        prop_assert!(result.is_err(),
            "string->integer should error for non-numeric string {:?}", s);
    }

    // =========================================================================
    // Index/char-at operations (boundary checking)
    // =========================================================================

    #[test]
    fn char_at_valid_index(s in "[a-z]{1,20}", idx in 0usize..20) {
        let idx = idx.min(s.len() - 1);
        let code = format!("(string/char-at \"{}\" {})", s, idx);
        let result = eval_source(&code).unwrap();
        let char_str = result.with_string(|s| s.to_string()).unwrap();
        // Should be a single character
        prop_assert_eq!(char_str.chars().count(), 1,
            "char-at should return single character");
    }

    #[test]
    fn char_at_out_of_bounds_errors(s in "[a-z]{1,10}") {
        let code = format!("(string/char-at \"{}\" 1000)", s);
        let result = eval_source(&code);
        // Should error on out-of-bounds
        prop_assert!(result.is_err(),
            "char-at should error on out-of-bounds index");
    }

    #[test]
    fn string_index_finds_char(s in "[a-z]{2,20}", needle in "[a-z]") {
        if s.contains(&needle.to_string()) {
            let code = format!("(string/index \"{}\" \"{}\")", s, needle);
            let result = eval_source(&code).unwrap();
            // Should return an integer index (not nil)
            prop_assert!(result.as_int().is_some(),
                "string/index should find character {:?} in {:?}", needle, s);
        }
    }

    #[test]
    fn string_index_not_found_returns_nil(s in "[a-z]{1,10}") {
        let code = format!("(string/index \"{}\" \"z\")", s);
        let result = eval_source(&code).unwrap();
        // If "z" is not in s, should return nil
        if !s.contains("z") {
            prop_assert_eq!(result, Value::NIL,
                "string/index should return nil when not found");
        }
    }

    // =========================================================================
    // Unicode-specific (multi-byte boundary handling)
    // =========================================================================

    #[test]
    fn unicode_append_preserves_content(a in arb_unicode_string(), b in arb_unicode_string()) {
        let a_esc = escape_for_elle(&a);
        let b_esc = escape_for_elle(&b);
        let code = format!(
            "(append \"{}\" \"{}\")",
            a_esc, b_esc
        );
        if let Ok(result) = eval_source(&code) {
            let result_str = result.with_string(|s| s.to_string()).unwrap();
            let expected = format!("{}{}", a, b);
            prop_assert_eq!(result_str, expected,
                "unicode append didn't preserve content for {:?} + {:?}", a, b);
        }
    }

    #[test]
    fn unicode_upcase_downcase_roundtrip(s in "[a-z]{1,15}") {
        // ASCII-only for now to avoid unicode case folding complexity
        let code = format!("(string/downcase (string/upcase \"{}\"))", s);
        if let Ok(result) = eval_source(&code) {
            prop_assert_eq!(result.with_string(|s| s.to_string()).unwrap(), s,
                "unicode upcase/downcase roundtrip failed");
        }
    }
}
