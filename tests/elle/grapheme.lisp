(elle/epoch 12)
## Grapheme cluster canaries
##
## length, get, slice, and reverse count UAX #29 extended grapheme
## clusters, so the Unicode segmentation tables are part of the language
## definition. These asserts pin the exact cluster counts the embedded
## tables produce for the constructs Unicode has revised most often:
## emoji ZWJ sequences, regional indicators, and Indic conjuncts.
## When one of these asserts fails, the segmentation tables changed;
## review the unicode-segmentation pin in Cargo.toml, these counts,
## and docs/strings.md together.

# ============================================================================
# CRLF (rule GB3: CR x LF never splits)
# ============================================================================

# lib/http.lisp and lib/irc.lisp strip a line terminator by dropping one
# trailing grapheme; that is sound only while CRLF is a single cluster.
(assert (= (length "\r\n") 1) "CRLF is one grapheme cluster")
(assert (= (length "a\r\nb") 3) "embedded CRLF counts as one cluster")
(assert (= (get "a\r\nb" 1) "\r\n") "get returns the whole CRLF cluster")
(assert (= (slice "hi\r\n" 0 (dec (length "hi\r\n"))) "hi")
        "dropping the final grapheme removes the whole CRLF")

# ============================================================================
# Emoji modifiers and ZWJ sequences
# ============================================================================

(assert (= (length "👋🏽") 1) "skin-tone modifier joins its base")
(assert (= (string/size-of "👋🏽") 8) "modifier sequence is 8 UTF-8 bytes")
(assert (= (length "👨‍👩‍👧‍👦") 1)
        "family ZWJ sequence is one cluster")
(assert (= (get "a👨‍👩‍👧‍👦b" 1) "👨‍👩‍👧‍👦")
        "get returns the whole ZWJ sequence")
(assert (= (length "🏳️‍🌈") 1)
        "flag + VS16 + ZWJ + rainbow is one cluster")
(assert (= (reverse "ab👨‍👩‍👧‍👦") "👨‍👩‍👧‍👦ba")
        "reverse keeps ZWJ sequences intact")

# ============================================================================
# Regional indicators
# ============================================================================

(assert (= (length "🇺🇸") 1) "one flag is one cluster")
(assert (= (length "🇺🇸🇫🇷") 2)
        "adjacent flags pair into two clusters")
(assert (= (get "🇺🇸🇫🇷" 1) "🇫🇷")
        "cluster boundaries fall between flag pairs")

# ============================================================================
# Indic conjuncts (rule GB9c, present since Unicode 15.1)
# ============================================================================

(assert (= (length "क्ष") 1)
        "KA + virama + SSA forms one conjunct cluster")
(assert (= (length "नमस्ते") 3)
        "namaste segments into three clusters")
(assert (= (get "नमस्ते" 2) "स्ते")
        "conjunct plus vowel sign is the final cluster")

# ============================================================================
# Decomposed combining marks
# ============================================================================

# The e below is followed by U+0301 COMBINING ACUTE ACCENT (not precomposed).
(assert (= (string/size-of "é") 3) "decomposed e + combining acute is 3 bytes")
(assert (= (length "é") 1) "combining mark joins its base")
