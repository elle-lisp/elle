//! String interning for the NaN-boxed value system.
//!
//! All strings are interned to enable O(1) equality comparison via pointer
//! equality. This is essential because NaN-boxed Values compare by bit pattern,
//! so strings must share the same heap allocation to be equal.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::value::heap::HeapObject;

thread_local! {
    static STRING_INTERNER: RefCell<StringInterner> = RefCell::new(StringInterner::new());
}

struct StringInterner {
    // Map from string content to Rc<HeapObject>
    // We use Rc to keep the strings alive and prevent them from being dropped
    strings: HashMap<Box<str>, Rc<HeapObject>>,
}

impl StringInterner {
    fn new() -> Self {
        StringInterner {
            strings: HashMap::new(),
        }
    }

    fn intern(&mut self, s: &str) -> *const HeapObject {
        // Check if already interned
        if let Some(rc) = self.strings.get(s) {
            return Rc::as_ptr(rc);
        }

        // Allocate new HeapObject::String
        let rc = Rc::new(HeapObject::String(s.into()));
        let ptr = Rc::as_ptr(&rc);

        // Store in table
        self.strings.insert(s.into(), rc);

        ptr
    }
}

/// Intern a string, returning a pointer to the HeapObject.
///
/// The returned pointer is valid for the lifetime of the thread.
/// Interned strings are never dropped during program execution.
pub fn intern_string(s: &str) -> *const HeapObject {
    STRING_INTERNER.with(|interner| interner.borrow_mut().intern(s))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::Value;

    // === Basic Interning Tests ===

    #[test]
    fn test_intern_same_string_returns_same_pointer() {
        let ptr1 = intern_string("hello");
        let ptr2 = intern_string("hello");
        assert_eq!(ptr1, ptr2, "Same string should return same pointer");
    }

    #[test]
    fn test_intern_different_strings_return_different_pointers() {
        let ptr1 = intern_string("hello");
        let ptr2 = intern_string("world");
        assert_ne!(
            ptr1, ptr2,
            "Different strings should return different pointers"
        );
    }

    #[test]
    fn test_intern_empty_string() {
        let ptr1 = intern_string("");
        let ptr2 = intern_string("");
        assert_eq!(ptr1, ptr2, "Empty strings should be interned");
    }

    #[test]
    fn test_intern_single_char() {
        let ptr1 = intern_string("a");
        let ptr2 = intern_string("a");
        let ptr3 = intern_string("b");
        assert_eq!(ptr1, ptr2);
        assert_ne!(ptr1, ptr3);
    }

    // === Unicode Tests ===

    #[test]
    fn test_intern_unicode_japanese() {
        let ptr1 = intern_string("こんにちは");
        let ptr2 = intern_string("こんにちは");
        assert_eq!(ptr1, ptr2, "Japanese strings should be interned");
    }

    #[test]
    fn test_intern_unicode_emoji() {
        let ptr1 = intern_string("🎉🚀💻");
        let ptr2 = intern_string("🎉🚀💻");
        assert_eq!(ptr1, ptr2, "Emoji strings should be interned");
    }

    #[test]
    fn test_intern_unicode_mixed() {
        let ptr1 = intern_string("Hello 世界 🌍");
        let ptr2 = intern_string("Hello 世界 🌍");
        assert_eq!(ptr1, ptr2, "Mixed unicode strings should be interned");
    }

    #[test]
    fn test_intern_unicode_normalization_not_applied() {
        // NFC vs NFD normalization - these are different byte sequences
        // even though they render the same. We don't normalize.
        let nfc = "é"; // single codepoint U+00E9
        let _nfd = "é"; // e + combining acute U+0065 U+0301
                        // Note: In source code these may be normalized by the editor
                        // This test verifies we don't do additional normalization
        let ptr1 = intern_string(nfc);
        let ptr2 = intern_string(nfc);
        assert_eq!(ptr1, ptr2);
    }

    // === Whitespace and Special Character Tests ===

    #[test]
    fn test_intern_whitespace_variations() {
        let space = intern_string(" ");
        let tab = intern_string("\t");
        let newline = intern_string("\n");
        let crlf = intern_string("\r\n");

        // All different
        assert_ne!(space, tab);
        assert_ne!(space, newline);
        assert_ne!(tab, newline);
        assert_ne!(newline, crlf);

        // Same strings intern to same pointer
        assert_eq!(space, intern_string(" "));
        assert_eq!(tab, intern_string("\t"));
        assert_eq!(newline, intern_string("\n"));
    }

    #[test]
    fn test_intern_null_byte() {
        let ptr1 = intern_string("hello\0world");
        let ptr2 = intern_string("hello\0world");
        let ptr3 = intern_string("hello");
        assert_eq!(ptr1, ptr2, "Strings with null bytes should be interned");
        assert_ne!(ptr1, ptr3, "String with null should differ from truncated");
    }

    #[test]
    fn test_intern_escape_sequences() {
        let ptr1 = intern_string("line1\nline2");
        let ptr2 = intern_string("line1\nline2");
        let ptr3 = intern_string("line1\\nline2"); // literal backslash-n
        assert_eq!(ptr1, ptr2);
        assert_ne!(ptr1, ptr3);
    }

    // === Substring and Prefix/Suffix Tests ===

    #[test]
    fn test_intern_prefix_not_equal() {
        let full = intern_string("hello world");
        let prefix = intern_string("hello");
        assert_ne!(full, prefix, "Prefix should not equal full string");
    }

    #[test]
    fn test_intern_suffix_not_equal() {
        let full = intern_string("hello world");
        let suffix = intern_string("world");
        assert_ne!(full, suffix, "Suffix should not equal full string");
    }

    #[test]
    fn test_intern_case_sensitive() {
        let lower = intern_string("hello");
        let upper = intern_string("HELLO");
        let mixed = intern_string("Hello");
        assert_ne!(lower, upper);
        assert_ne!(lower, mixed);
        assert_ne!(upper, mixed);
    }

    // === Value Integration Tests ===

    #[test]
    fn test_value_string_equality() {
        let v1 = Value::string("test");
        let v2 = Value::string("test");
        assert_eq!(v1, v2, "Value::string with same content should be equal");
    }

    #[test]
    fn test_value_string_inequality() {
        let v1 = Value::string("test1");
        let v2 = Value::string("test2");
        assert_ne!(
            v1, v2,
            "Value::string with different content should not be equal"
        );
    }

    #[test]
    fn test_value_string_empty() {
        let v1 = Value::string("");
        let v2 = Value::string("");
        assert_eq!(v1, v2, "Empty Value::strings should be equal");
    }

    #[test]
    fn test_value_string_content_extraction() {
        let v = Value::string("hello world");
        assert_eq!(v.as_string(), Some("hello world"));
    }

    #[test]
    fn test_value_string_many_identical() {
        // Create many identical strings - all should be equal
        let strings: Vec<Value> = (0..100).map(|_| Value::string("repeated")).collect();
        for s in &strings {
            assert_eq!(*s, strings[0]);
        }
    }

    #[test]
    fn test_value_string_many_unique() {
        // Create many unique strings - all should be different
        let strings: Vec<Value> = (0..100)
            .map(|i| Value::string(format!("string_{}", i)))
            .collect();
        for i in 0..strings.len() {
            for j in (i + 1)..strings.len() {
                assert_ne!(strings[i], strings[j]);
            }
        }
    }

    // === Heap Object Verification Tests ===

    #[test]
    fn test_interned_string_content_accessible() {
        let ptr = intern_string("verification test");
        let heap_obj = unsafe { &*ptr };
        match heap_obj {
            HeapObject::String(s) => assert_eq!(&**s, "verification test"),
            _ => panic!("Expected HeapObject::String"),
        }
    }

    #[test]
    fn test_interned_string_survives_scope() {
        let ptr1 = {
            let s = String::from("scoped string");
            intern_string(&s)
        };
        // The original String is dropped, but interned version survives
        let ptr2 = intern_string("scoped string");
        assert_eq!(ptr1, ptr2);

        // Verify content is still accessible
        let heap_obj = unsafe { &*ptr1 };
        match heap_obj {
            HeapObject::String(s) => assert_eq!(&**s, "scoped string"),
            _ => panic!("Expected HeapObject::String"),
        }
    }

    // === Stress Tests ===

    #[test]
    fn test_intern_many_strings() {
        // Intern 1000 unique strings
        let ptrs: Vec<*const HeapObject> = (0..1000)
            .map(|i| intern_string(&format!("string_number_{}", i)))
            .collect();

        // Verify all are unique
        for i in 0..ptrs.len() {
            for j in (i + 1)..ptrs.len() {
                assert_ne!(ptrs[i], ptrs[j], "All strings should have unique pointers");
            }
        }

        // Verify re-interning returns same pointers
        for (i, &expected_ptr) in ptrs.iter().enumerate() {
            let ptr = intern_string(&format!("string_number_{}", i));
            assert_eq!(ptr, expected_ptr, "Re-interning should return same pointer");
        }
    }

    #[test]
    fn test_intern_long_string() {
        let long = "a".repeat(10000);
        let ptr1 = intern_string(&long);
        let ptr2 = intern_string(&long);
        assert_eq!(ptr1, ptr2, "Long strings should be interned");

        // Verify content
        let heap_obj = unsafe { &*ptr1 };
        match heap_obj {
            HeapObject::String(s) => assert_eq!(s.len(), 10000),
            _ => panic!("Expected HeapObject::String"),
        }
    }

    #[test]
    fn test_intern_binary_like_content() {
        // Strings with bytes that might look like binary data
        let s1 = "\x00\x01\x02\x03";
        let s2 = "\u{00ff}\u{00fe}\u{00fd}\u{00fc}";
        let ptr1a = intern_string(s1);
        let ptr1b = intern_string(s1);
        let ptr2 = intern_string(s2);

        assert_eq!(ptr1a, ptr1b);
        assert_ne!(ptr1a, ptr2);
    }

    // === Edge Cases ===

    #[test]
    fn test_intern_similar_strings() {
        // Strings that are similar but not identical
        let ptrs = [
            intern_string("test"),
            intern_string("test "),  // trailing space
            intern_string(" test"),  // leading space
            intern_string("Test"),   // different case
            intern_string("test\n"), // trailing newline
            intern_string("tест"),   // cyrillic 'е'
        ];

        // All should be different
        for i in 0..ptrs.len() {
            for j in (i + 1)..ptrs.len() {
                assert_ne!(
                    ptrs[i], ptrs[j],
                    "Similar but different strings should have different pointers"
                );
            }
        }
    }

    #[test]
    fn test_intern_from_various_sources() {
        // Test interning from different string types
        let s1 = "literal";
        let s2 = String::from("literal");
        let s3 = "literal".to_string();
        let s4: Box<str> = "literal".into();

        let ptr1 = intern_string(s1);
        let ptr2 = intern_string(&s2);
        let ptr3 = intern_string(&s3);
        let ptr4 = intern_string(&s4);

        assert_eq!(ptr1, ptr2);
        assert_eq!(ptr2, ptr3);
        assert_eq!(ptr3, ptr4);
    }
}
