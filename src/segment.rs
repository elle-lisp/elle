//! Unicode grapheme segmentation seam.
//!
//! String semantics — `length`, `get`, `slice`, iteration — count UAX #29
//! extended grapheme clusters, so the segmentation tables are part of the
//! language definition. Each build vendors one or more table generations.
//! Every VM selects one generation at construction and keeps it for its
//! whole life: text ports stash bytes split at cluster boundaries, so a
//! mid-run table change would corrupt their framing.
//!
//! G17 is the crates.io `unicode-segmentation` dependency pinned in
//! Cargo.toml. G16 is a frozen vendored copy under `segment/g16/`.
//! Rolling the newest generation means bumping the Cargo pin, adding the
//! previous tables as a new vendored module, extending [`Generation`],
//! and updating the tests here plus tests/elle/{grapheme,unicode}.lisp
//! and docs/strings.md.

mod g16;

use crate::reader::read_syntax_all;
use crate::syntax::SyntaxKind;
use unicode_segmentation::UnicodeSegmentation;

/// One vendored UAX #29 table set. Per-VM, fixed at construction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Generation {
    /// Unicode 16.0.0 (vendored from unicode-segmentation 1.12.0).
    G16,
    /// Unicode 17.0.0 (the crates.io dependency).
    G17,
}

impl Generation {
    /// The default for every VM that does not select otherwise.
    pub const NEWEST: Generation = Generation::G17;

    /// All vendored generations, oldest first.
    pub const ALL: &'static [Generation] = &[Generation::G16, Generation::G17];

    /// The Unicode version of this generation's tables.
    pub fn version(self) -> (u64, u64, u64) {
        match self {
            Generation::G16 => g16::tables::UNICODE_VERSION,
            Generation::G17 => unicode_segmentation::UNICODE_VERSION,
        }
    }

    /// The version in "17.0.0" form, for messages.
    pub fn version_string(self) -> String {
        let (major, minor, patch) = self.version();
        format!("{}.{}.{}", major, minor, patch)
    }

    /// Resolve a 1-3 component version request against the vendored
    /// generations by prefix match: `[17]` accepts 17.x.x, `[17, 0]`
    /// accepts 17.0.x. The error lists what this build vendors.
    pub fn from_request(request: &[i64]) -> Result<Generation, String> {
        debug_assert!((1..=3).contains(&request.len()));
        for generation in Self::ALL {
            let (major, minor, patch) = generation.version();
            let full = [major as i64, minor as i64, patch as i64];
            if request.iter().zip(full.iter()).all(|(r, f)| r == f) {
                return Ok(*generation);
            }
        }
        let requested: Vec<String> = request.iter().map(|c| c.to_string()).collect();
        let vendored: Vec<String> = Self::ALL.iter().map(|g| g.version_string()).collect();
        Err(format!(
            "Unicode {} is not available in this build; vendored generations: {}",
            requested.join("."),
            vendored.join(", ")
        ))
    }
}

/// The extended grapheme clusters of `s` under `gen`.
pub fn graphemes(s: &str, gen: Generation) -> Graphemes<'_> {
    match gen {
        Generation::G16 => Graphemes::G16(g16::grapheme::new_graphemes(s, true)),
        Generation::G17 => Graphemes::G17(s.graphemes(true)),
    }
}

/// The number of extended grapheme clusters in `s` under `gen`.
pub fn grapheme_count(s: &str, gen: Generation) -> usize {
    graphemes(s, gen).count()
}

/// Enum-dispatch cluster iterator over the vendored generations.
#[derive(Clone)]
pub enum Graphemes<'a> {
    G16(g16::grapheme::Graphemes<'a>),
    G17(unicode_segmentation::Graphemes<'a>),
}

impl<'a> Iterator for Graphemes<'a> {
    type Item = &'a str;

    #[inline]
    fn next(&mut self) -> Option<&'a str> {
        match self {
            Graphemes::G16(inner) => inner.next(),
            Graphemes::G17(inner) => inner.next(),
        }
    }
}

impl<'a> DoubleEndedIterator for Graphemes<'a> {
    #[inline]
    fn next_back(&mut self) -> Option<&'a str> {
        match self {
            Graphemes::G16(inner) => inner.next_back(),
            Graphemes::G17(inner) => inner.next_back(),
        }
    }
}

/// Pre-scan the main file for top-level `(unicode! …)` declarations, before
/// any VM exists. Returns the agreed request (the longest declared prefix),
/// or an error when two declarations disagree. Non-integer components are
/// left for the analyzer to reject with a proper span.
pub fn scan_unicode_request(source: &str, source_name: &str) -> Result<Option<Vec<i64>>, String> {
    // Runs before any VM exists, so it brings its own heap.
    let mut home = crate::syntax::SyntaxHeap::new();
    let forms = read_syntax_all(home.arena(), source, source_name)?;
    let mut agreed: Option<Vec<i64>> = None;
    for form in &forms {
        let SyntaxKind::List(items) = &form.kind else {
            continue;
        };
        if items.is_empty() || !items[0].is_symbol("unicode!") || items.len() > 4 {
            continue;
        }
        let mut request = Vec::new();
        for item in &items[1..] {
            match item.kind {
                SyntaxKind::Int(n) if n >= 0 => request.push(n),
                _ => return Ok(agreed),
            }
        }
        if request.is_empty() {
            // 0-arg form is the query, not a declaration.
            continue;
        }
        match &agreed {
            None => agreed = Some(request),
            Some(prior) => {
                let shorter = prior.len().min(request.len());
                if prior[..shorter] != request[..shorter] {
                    return Err(format!(
                        "conflicting unicode! declarations at {}: {} was already declared, then {}",
                        form.span,
                        prior
                            .iter()
                            .map(|c| c.to_string())
                            .collect::<Vec<_>>()
                            .join("."),
                        request
                            .iter()
                            .map(|c| c.to_string())
                            .collect::<Vec<_>>()
                            .join(".")
                    ));
                }
                if request.len() > prior.len() {
                    agreed = Some(request);
                }
            }
        }
    }
    Ok(agreed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole language defines string semantics in grapheme clusters,
    /// so each generation's table version is part of the language spec.
    /// When this fails, a dependency bump changed the newest tables:
    /// vendor the previous tables as a new generation and update
    /// Generation, tests/elle/{grapheme,unicode}.lisp, and
    /// docs/strings.md together.
    #[test]
    fn newest_matches_dep() {
        assert_eq!(Generation::G17.version(), (17, 0, 0));
        assert_eq!(Generation::G16.version(), (16, 0, 0));
        assert_eq!(Generation::NEWEST, Generation::G17);
        assert_eq!(unicode_segmentation::UNICODE_VERSION, (17, 0, 0));
    }

    /// U+10EFA: GC_Extend in the 17.0 grapheme table (range starts at
    /// U+10EFA), unassigned in 16.0 (range starts at U+10EFC). Verified
    /// with: grep -o "'.u{10ef.*GC_" src/segment/g16/tables.rs vs the
    /// 1.13.2 crate tables.
    #[test]
    fn generations_diverge_on_new_extend() {
        let s = "a\u{10EFA}";
        assert_eq!(grapheme_count(s, Generation::G16), 2);
        assert_eq!(grapheme_count(s, Generation::G17), 1);
    }

    /// Constructs whose segmentation both generations define identically
    /// (both are >= Unicode 15.1 semantics, including GB9c conjuncts).
    #[test]
    fn generations_agree_on_basics() {
        for s in [
            "\r\n",
            "👨\u{200D}👩\u{200D}👧\u{200D}👦",
            "🇺🇸🇫🇷",
            "क्ष",
            "नमस्ते",
            "café",
        ] {
            assert_eq!(
                grapheme_count(s, Generation::G16),
                grapheme_count(s, Generation::G17),
                "generations must agree on {:?}",
                s
            );
        }
        assert_eq!(grapheme_count("🇺🇸🇫🇷", Generation::G16), 2);
        assert_eq!(grapheme_count("नमस्ते", Generation::G16), 3);
    }

    #[test]
    fn double_ended_matches_forward() {
        for s in ["a\u{10EFA}b🇺🇸🇫🇷", "नमस्ते\r\nx"] {
            for gen in Generation::ALL {
                let forward: Vec<&str> = graphemes(s, *gen).collect();
                let mut backward: Vec<&str> = graphemes(s, *gen).rev().collect();
                backward.reverse();
                assert_eq!(forward, backward, "under {:?}", gen);
            }
        }
    }

    #[test]
    fn from_request_prefix_matches() {
        assert_eq!(Generation::from_request(&[16]), Ok(Generation::G16));
        assert_eq!(Generation::from_request(&[16, 0]), Ok(Generation::G16));
        assert_eq!(Generation::from_request(&[17]), Ok(Generation::G17));
        assert_eq!(Generation::from_request(&[17, 0, 0]), Ok(Generation::G17));
        let err = Generation::from_request(&[15]).unwrap_err();
        assert!(err.contains("not available in this build"), "{}", err);
        assert!(err.contains("16.0.0") && err.contains("17.0.0"), "{}", err);
        assert!(Generation::from_request(&[17, 1]).is_err());
    }

    #[test]
    fn scan_finds_and_reconciles_declarations() {
        assert_eq!(scan_unicode_request("(def x 1)", "<t>").unwrap(), None);
        assert_eq!(
            scan_unicode_request("(unicode! 16)\n(def x 1)", "<t>").unwrap(),
            Some(vec![16])
        );
        // The longest agreeing prefix wins; the query form is ignored.
        assert_eq!(
            scan_unicode_request("(unicode! 17)\n(unicode!)\n(unicode! 17 0)", "<t>").unwrap(),
            Some(vec![17, 0])
        );
        assert!(scan_unicode_request("(unicode! 16)\n(unicode! 17)", "<t>").is_err());
    }
}
