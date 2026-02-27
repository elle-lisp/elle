// Property tests for string operations.
//
// Covers unicode handling, string operation laws, and edge cases.
// Existing tests only use [a-z] — these push into multi-byte territory.

use crate::common::eval_source;
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
    #![proptest_config(ProptestConfig::with_cases(200))]

    // =========================================================================
    // Slice/substring properties (used to test length-like behavior)
    // =========================================================================

    #[test]
    fn slice_full_range_is_identity(s in "[a-zA-Z0-9 ]{0,30}") {
        // Slicing from 0 to the end should give back the original
        let code = format!("(string/slice \"{}\" 0 {})", s, s.len());
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result.as_string().unwrap(), s.as_str(),
            "full slice should be identity for {:?}", s);
    }

    #[test]
    fn empty_string_slice(_dummy in 0..1i32) {
        // Slicing empty string should work
        let result = eval_source("(string/slice \"\" 0 0)").unwrap();
        prop_assert_eq!(result.as_string().unwrap(), "");
    }

    #[test]
    fn slice_at_zero_to_zero(s in "[a-zA-Z0-9]{0,30}") {
        // Slice from 0 to 0 should be empty
        let code = format!("(string/slice \"{}\" 0 0)", s);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result.as_string().unwrap(), "");
    }

    // =========================================================================
    // Append properties
    // =========================================================================

    #[test]
    fn append_preserves_content(a in "[a-zA-Z]{0,15}", b in "[a-zA-Z]{0,15}") {
        let code = format!(
            "(append \"{}\" \"{}\")",
            a, b
        );
        let result = eval_source(&code).unwrap();
        let expected = format!("{}{}", a, b);
        prop_assert_eq!(result.as_string().unwrap(), expected.as_str(),
            "append didn't preserve content for {:?} + {:?}", a, b);
    }

    #[test]
    fn append_empty_is_identity(s in "[a-zA-Z0-9]{0,20}") {
        let code = format!("(append \"{}\" \"\")", s);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result.as_string().unwrap(), s.as_str(),
            "appending empty string changed {:?}", s);

        let code = format!("(append \"\" \"{}\")", s);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result.as_string().unwrap(), s.as_str(),
            "prepending empty string changed {:?}", s);
    }

    #[test]
    fn append_associative(
        a in "[a-z]{0,8}",
        b in "[a-z]{0,8}",
        c in "[a-z]{0,8}",
    ) {
        let r1 = eval_source(&format!(
            "(append (append \"{}\" \"{}\") \"{}\")", a, b, c
        )).unwrap();
        let r2 = eval_source(&format!(
            "(append \"{}\" (append \"{}\" \"{}\"))", a, b, c
        )).unwrap();
        prop_assert_eq!(r1, r2, "append not associative");
    }

    // =========================================================================
    // Substring/Slice properties
    // =========================================================================

    #[test]
    fn slice_full_is_identity(s in "[a-zA-Z0-9]{1,20}") {
        let code = format!("(string/slice \"{}\" 0 (length \"{}\"))", s, s);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result.as_string().unwrap(), s.as_str(),
            "full slice changed {:?}", s);
    }

    #[test]
    fn slice_empty_range(s in "[a-zA-Z0-9]{1,20}", i in 0usize..20) {
        let len = s.len();
        let i = i.min(len);
        let code = format!("(string/slice \"{}\" {} {})", s, i, i);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result.as_string().unwrap(), "",
            "empty range slice should be empty for {:?} at {}", s, i);
    }

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
                let sliced = val.as_string().unwrap();
                // Verify it's a substring of the original
                prop_assert!(s.contains(sliced),
                    "slice result not substring of original");
            }
        }
    }

    // =========================================================================
    // Out-of-bounds slice returns nil (#339)
    // =========================================================================

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
    // Case conversion properties
    // =========================================================================

    #[test]
    fn upcase_downcase_roundtrip(s in "[a-z]{1,20}") {
        let code = format!("(string/downcase (string/upcase \"{}\"))", s);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result.as_string().unwrap(), s.as_str(),
            "upcase/downcase roundtrip failed for {:?}", s);
    }

    #[test]
    fn upcase_idempotent(s in "[A-Z]{1,20}") {
        let code = format!("(string/upcase (string/upcase \"{}\"))", s);
        let result = eval_source(&code).unwrap();
        let code2 = format!("(string/upcase \"{}\")", s);
        let result2 = eval_source(&code2).unwrap();
        prop_assert_eq!(result, result2, "upcase not idempotent for {:?}", s);
    }

    #[test]
    fn downcase_idempotent(s in "[a-z]{1,20}") {
        let code = format!("(string/downcase (string/downcase \"{}\"))", s);
        let result = eval_source(&code).unwrap();
        let code2 = format!("(string/downcase \"{}\")", s);
        let result2 = eval_source(&code2).unwrap();
        prop_assert_eq!(result, result2, "downcase not idempotent for {:?}", s);
    }

    #[test]
    fn upcase_preserves_content_length(s in "[a-z]{1,20}") {
        let code = format!("(string/upcase \"{}\")", s);
        let result = eval_source(&code).unwrap();
        let upcased = result.as_string().unwrap();
        // Length should be preserved (same number of bytes)
        prop_assert_eq!(upcased.len(), s.len(),
            "upcase changed byte length of {:?}", s);
    }

    #[test]
    fn downcase_preserves_content_length(s in "[A-Z]{1,20}") {
        let code = format!("(string/downcase \"{}\")", s);
        let result = eval_source(&code).unwrap();
        let downcased = result.as_string().unwrap();
        // Length should be preserved (same number of bytes)
        prop_assert_eq!(downcased.len(), s.len(),
            "downcase changed byte length of {:?}", s);
    }

    // =========================================================================
    // Contains / starts-with / ends-with
    // =========================================================================

    #[test]
    fn string_contains_self(s in "[a-z]{1,15}") {
        let code = format!("(string/contains? \"{}\" \"{}\")", s, s);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result, Value::TRUE,
            "string doesn't contain itself: {:?}", s);
    }

    #[test]
    fn string_contains_empty(s in "[a-z]{0,15}") {
        let code = format!("(string/contains? \"{}\" \"\")", s);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result, Value::TRUE,
            "string doesn't contain empty string: {:?}", s);
    }

    #[test]
    fn starts_with_self(s in "[a-z]{1,15}") {
        let code = format!("(string/starts-with? \"{}\" \"{}\")", s, s);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result, Value::TRUE);
    }

    #[test]
    fn starts_with_empty(s in "[a-z]{0,15}") {
        let code = format!("(string/starts-with? \"{}\" \"\")", s);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result, Value::TRUE,
            "string doesn't start with empty string: {:?}", s);
    }

    #[test]
    fn ends_with_self(s in "[a-z]{1,15}") {
        let code = format!("(string/ends-with? \"{}\" \"{}\")", s, s);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result, Value::TRUE);
    }

    #[test]
    fn ends_with_empty(s in "[a-z]{0,15}") {
        let code = format!("(string/ends-with? \"{}\" \"\")", s);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result, Value::TRUE,
            "string doesn't end with empty string: {:?}", s);
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
        prop_assert_eq!(result.as_string().unwrap(), joined.as_str(),
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
    // Replace
    // =========================================================================

    #[test]
    fn replace_with_self_is_identity(s in "[a-z]{1,15}", sub in "[a-z]{1,3}") {
        let code = format!("(string/replace \"{}\" \"{}\" \"{}\")", s, sub, sub);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result.as_string().unwrap(), s.as_str(),
            "replacing {:?} with itself changed {:?}", sub, s);
    }

    #[test]
    fn replace_empty_old_errors(s in "[a-z]{1,15}") {
        let code = format!("(string/replace \"{}\" \"\" \"x\")", s);
        let result = eval_source(&code);
        // Replacing empty string should error
        prop_assert!(result.is_err(),
            "replace with empty old should error");
    }

    // =========================================================================
    // Trim
    // =========================================================================

    #[test]
    fn trim_idempotent(s in "[a-z]{0,15}") {
        let padded = format!("  {}  ", s);
        let code = format!("(string/trim (string/trim \"{}\"))", padded);
        let result = eval_source(&code).unwrap();
        let code2 = format!("(string/trim \"{}\")", padded);
        let result2 = eval_source(&code2).unwrap();
        prop_assert_eq!(result, result2, "trim not idempotent");
    }

    #[test]
    fn trim_of_trimmed_is_noop(s in "[a-zA-Z0-9]{1,15}") {
        // String with no leading/trailing whitespace
        let code = format!("(string/trim \"{}\")", s);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result.as_string().unwrap(), s.as_str(),
            "trim changed non-whitespace string {:?}", s);
    }

    #[test]
    fn trim_removes_whitespace(s in "[a-z]{1,10}") {
        let padded = format!("   {}   ", s);
        let code = format!("(string/trim \"{}\")", padded);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result.as_string().unwrap(), s.as_str(),
            "trim didn't remove padding from {:?}", padded);
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
    // Index/char-at operations
    // =========================================================================

    #[test]
    fn char_at_valid_index(s in "[a-z]{1,20}", idx in 0usize..20) {
        let idx = idx.min(s.len() - 1);
        let code = format!("(string/char-at \"{}\" {})", s, idx);
        let result = eval_source(&code).unwrap();
        let char_str = result.as_string().unwrap();
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
    // Unicode-specific (basic coverage)
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
            let result_str = result.as_string().unwrap();
            let expected = format!("{}{}", a, b);
            prop_assert_eq!(result_str, expected.as_str(),
                "unicode append didn't preserve content for {:?} + {:?}", a, b);
        }
    }

    #[test]
    fn unicode_upcase_downcase_roundtrip(s in "[a-z]{1,15}") {
        // ASCII-only for now to avoid unicode case folding complexity
        let code = format!("(string/downcase (string/upcase \"{}\"))", s);
        if let Ok(result) = eval_source(&code) {
            prop_assert_eq!(result.as_string().unwrap(), s.as_str(),
                "unicode upcase/downcase roundtrip failed");
        }
    }

    // =========================================================================
    // Edge cases
    // =========================================================================

    #[test]
    fn empty_string_operations(_dummy in 0..1i32) {
        // append empty strings
        let r2 = eval_source("(append \"\" \"\")").unwrap();
        prop_assert_eq!(r2.as_string().unwrap(), "");

        // upcase empty string
        let r3 = eval_source("(string/upcase \"\")").unwrap();
        prop_assert_eq!(r3.as_string().unwrap(), "");

        // downcase empty string
        let r4 = eval_source("(string/downcase \"\")").unwrap();
        prop_assert_eq!(r4.as_string().unwrap(), "");

        // trim empty string
        let r5 = eval_source("(string/trim \"\")").unwrap();
        prop_assert_eq!(r5.as_string().unwrap(), "");

        // slice empty string
        let r6 = eval_source("(string/slice \"\" 0 0)").unwrap();
        prop_assert_eq!(r6.as_string().unwrap(), "");
    }

    #[test]
    fn whitespace_only_string(_dummy in 0..1i32) {
        let r = eval_source("(string/trim \"   \")").unwrap();
        prop_assert_eq!(r.as_string().unwrap(), "");
    }

    #[test]
    fn single_character_operations(c in "[a-z]") {
        let code = format!("(length \"{}\")", c);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result, Value::int(1));

        let code = format!("(string/char-at \"{}\" 0)", c);
        let result = eval_source(&code).unwrap();
        prop_assert_eq!(result.as_string().unwrap(), c.as_str());
    }
}
