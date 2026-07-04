(elle/epoch 12)
# tests/elle/concat-linear.lisp — string concat must be linear.
#
# Counterfactual for the O(n²) string-concat regression introduced by
# commit 40f918aa ("make concat and push-all linear"): it rewrote
# `push-all` (src/core.lisp) to walk its source with `(get src i)`, but
# `get` on a *string* is `s.graphemes(true).nth(i)` = O(i), so
# `push-all` over an L-grapheme string is O(L²) and `(concat s s)` is
# O(L²).  `%string-push` already bulk-appends a whole string's bytes
# (intrinsics.rs), so a string concat MUST be linear.
#
# Pre-fix this file times out: it is the `port-read-exact` /
# `port-shortread-framing` smoke hang in miniature (both build a large
# payload string by doubling concat).  Arrays/bytes (`get` is O(1)) were
# never affected — only grapheme-clustered strings.

# Build a >100k-grapheme string by doubling (O(log n) concats).
(def big
  (let [@s "0123456789"]
    (while (< (length s) 100000) (assign s (concat s s)))
    s))

(assert (>= (length big) 100000) "built a large string by doubling concat")
(assert (= (slice big 0 10) "0123456789") "content preserved across doublings")

# A single concat of the large string must complete in linear time.
# Pre-fix this one concat is O((1.6e5)²) ≈ tens of seconds to minutes;
# post-fix it is a couple of byte-buffer extends, well under a second.
(def t0 (clock/monotonic))
(def doubled (concat big big))
(def elapsed (- (clock/monotonic) t0))

(assert (= (length doubled) (* 2 (length big))) "concat length is the sum")
(assert (= (slice doubled 0 10) "0123456789") "concat content head correct")
(assert (< elapsed 5.0)
        (concat "string concat must be linear; (concat big big) took "
                (string elapsed) "s (quadratic regression)"))

# The linear path bulk-appends a whole string via %string-push, which must
# accept BOTH an immutable string and a mutable @string as the pushed value
# (push-all / concat feed it the source collection directly). Exercise the
# @string-source cases that the per-grapheme walk used to mask.
# NOTE: concat with a *mutable first arg* appends in place and returns it,
# so each case uses fresh buffers to avoid cross-contamination.
(def @src (@string))
(%string-push src "abc")  # @string as a non-first (source) operand: not mutated, bulk-appended.
(assert (= (concat "xyz" src) "xyzabc") "concat string + @string source")
(assert (= src "abc") "@string source left unmutated when not first")
(def @dst (@string))
(%string-push dst src)  # push a whole @string value onto another
(assert (= (freeze dst) "abc") "%string-push accepts an @string value")
(def @a (@string))
(%string-push a "abc")
(def @b (@string))
(%string-push b "de")
(assert (= (concat a b) "abcde") "concat @string + @string source")

(println "concat-linear ok: |big|=" (length big) " concat took " elapsed "s")
