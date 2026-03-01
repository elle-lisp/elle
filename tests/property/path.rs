// Property tests for the path module.
//
// Verifies algebraic properties of path operations across
// generated inputs. Primary verification strategy for the path API.

use elle::path;
use proptest::prelude::*;

/// Strategy for generating a valid non-empty path component.
/// No slashes, no empty strings, no dots (avoid normalization interference).
fn arb_component() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9_-]{1,20}".prop_map(|s| s)
}

/// Strategy for a component that may have an extension.
fn arb_filename() -> impl Strategy<Value = String> {
    prop_oneof![
        // filename with extension
        ("[a-zA-Z0-9_-]{1,10}\\.[a-zA-Z0-9]{1,5}".prop_map(|s| s)),
        // filename without extension
        ("[a-zA-Z0-9_-]{1,10}".prop_map(|s| s)),
    ]
}

/// Strategy for an extension (non-empty, no dots, no slashes).
fn arb_extension() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9]{1,5}".prop_map(|s| s)
}

/// Strategy for a relative path (1-4 components).
fn arb_relative_path() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_component(), 1..=4).prop_map(|parts| {
        let refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
        path::join(&refs)
    })
}

/// Strategy for an absolute path (/ + 1-4 components).
fn arb_absolute_path() -> impl Strategy<Value = String> {
    prop::collection::vec(arb_component(), 1..=4).prop_map(|parts| {
        let mut refs: Vec<&str> = vec!["/"];
        for p in &parts {
            refs.push(p.as_str());
        }
        path::join(&refs)
    })
}

/// Strategy for any path (absolute or relative).
fn arb_path() -> impl Strategy<Value = String> {
    prop_oneof![arb_relative_path(), arb_absolute_path(),]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(1000))]

    // =========================================================================
    // join / parent roundtrip
    // =========================================================================

    #[test]
    fn join_then_parent_recovers_prefix(
        prefix in arb_path(),
        last in arb_component(),
    ) {
        let joined = path::join(&[&prefix, &last]);
        let parent = path::parent(&joined);
        prop_assert_eq!(
            parent,
            Some(prefix.as_str()),
            "parent(join([{:?}, {:?}])) = {:?}, expected Some({:?})",
            prefix, last, parent, prefix,
        );
    }

    // =========================================================================
    // join / filename roundtrip
    // =========================================================================

    #[test]
    fn join_then_filename_recovers_last(
        prefix in arb_path(),
        last in arb_component(),
    ) {
        let joined = path::join(&[&prefix, &last]);
        let fname = path::filename(&joined);
        prop_assert_eq!(
            fname,
            Some(last.as_str()),
            "filename(join([{:?}, {:?}])) = {:?}, expected Some({:?})",
            prefix, last, fname, last,
        );
    }

    // =========================================================================
    // with_extension / extension roundtrip
    // =========================================================================

    #[test]
    fn with_extension_then_extension_roundtrips(
        base in arb_component(),
        ext in arb_extension(),
    ) {
        let with_ext = path::with_extension(&base, &ext);
        let got = path::extension(&with_ext);
        prop_assert_eq!(
            got,
            Some(ext.as_str()),
            "extension(with_extension({:?}, {:?})) = {:?}",
            base, ext, got,
        );
    }

    // =========================================================================
    // with_extension preserves stem
    // =========================================================================

    #[test]
    fn with_extension_preserves_stem(
        base in arb_component(),
        ext in arb_extension(),
    ) {
        let original_stem = path::stem(&base);
        let with_ext = path::with_extension(&base, &ext);
        let new_stem = path::stem(&with_ext);
        prop_assert_eq!(
            new_stem, original_stem,
            "stem changed after with_extension: {:?} -> {:?}",
            base, with_ext,
        );
    }

    // =========================================================================
    // normalize is idempotent
    // =========================================================================

    #[test]
    fn normalize_idempotent(path in arb_path()) {
        let once = path::normalize(&path);
        let twice = path::normalize(&once);
        prop_assert_eq!(
            once, twice,
            "normalize is not idempotent for {:?}",
            path,
        );
    }

    // =========================================================================
    // is_absolute and is_relative are complementary
    // =========================================================================

    #[test]
    fn absolute_relative_complementary(path in arb_path()) {
        prop_assert_ne!(
            path::is_absolute(&path),
            path::is_relative(&path),
            "is_absolute and is_relative are not complementary for {:?}",
            path,
        );
    }

    // =========================================================================
    // components then join roundtrips (modulo normalization)
    // =========================================================================

    #[test]
    fn components_join_roundtrip(path in arb_path()) {
        let parts = path::components(&path);
        let refs: Vec<&str> = parts.iter().map(|s| s.as_str()).collect();
        let rejoined = path::join(&refs);
        prop_assert_eq!(
            path::normalize(&rejoined),
            path::normalize(&path),
            "components->join roundtrip failed for {:?} (parts: {:?})",
            path, parts,
        );
    }

    // =========================================================================
    // relative then join roundtrips for absolute paths
    // =========================================================================

    #[test]
    fn relative_join_roundtrip(
        target in arb_absolute_path(),
        base in arb_absolute_path(),
    ) {
        if let Some(rel) = path::relative(&target, &base) {
            let rejoined = path::join(&[base.as_str(), rel.as_str()]);
            prop_assert_eq!(
                path::normalize(&rejoined),
                path::normalize(&target),
                "relative->join roundtrip failed: target={:?}, base={:?}, rel={:?}",
                target, base, rel,
            );
        }
        // None case: no relative path exists, nothing to assert
    }

    // =========================================================================
    // join with absolute component replaces prefix
    // =========================================================================

    #[test]
    fn join_absolute_replaces(
        prefix in arb_relative_path(),
        abs in arb_absolute_path(),
    ) {
        let joined = path::join(&[&prefix, &abs]);
        prop_assert!(
            path::is_absolute(&joined),
            "join with absolute component should produce absolute path: {:?} + {:?} = {:?}",
            prefix, abs, joined,
        );
        prop_assert_eq!(
            joined, abs,
            "join with absolute should equal the absolute component",
        );
    }

    // =========================================================================
    // parent always produces a shorter or empty path
    // =========================================================================

    #[test]
    fn parent_is_shorter_or_empty(path in arb_path()) {
        if let Some(p) = path::parent(&path) {
            prop_assert!(
                p.len() < path.len(),
                "parent {:?} is not shorter than {:?}",
                p, path,
            );
        }
    }

    // =========================================================================
    // stem is always Some for filenames
    // =========================================================================

    #[test]
    fn stem_always_some_for_filename(name in arb_filename()) {
        let s = path::stem(&name);
        prop_assert!(
            s.is_some(),
            "stem({:?}) returned None",
            name,
        );
    }

    // =========================================================================
    // filename extension roundtrip via with_extension
    // =========================================================================

    #[test]
    fn filename_extension_roundtrip(name in arb_filename(), ext in arb_extension()) {
        let stem = path::stem(&name).unwrap();
        let with_ext = path::with_extension(&name, &ext);
        let new_stem = path::stem(&with_ext);
        let new_ext = path::extension(&with_ext);
        prop_assert_eq!(
            new_stem,
            Some(stem),
            "stem changed: {:?} -> with_extension({:?}) -> stem = {:?}",
            name, ext, new_stem,
        );
        prop_assert_eq!(
            new_ext,
            Some(ext.as_str()),
            "extension not recovered: with_extension({:?}, {:?}) -> extension = {:?}",
            name, ext, new_ext,
        );
    }
}
