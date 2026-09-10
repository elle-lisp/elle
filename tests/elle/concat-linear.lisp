(elle/epoch 12)
# audited: 2026-09-10
# tests/elle/concat-linear.lisp — string concat must be linear.
#
# Counterfactual for the O(n²) string-concat regression introduced by
# commit 40f918aa ("make concat and push-all linear"): it rewrote
# `push-all` (src/core.lisp) to walk its source with `(get src i)`, but
# `get` on a *string* is `segment::graphemes(s, gen).nth(i)` = O(i), so
# `push-all` over an L-grapheme string is O(L²) and `(concat s s)` is
# O(L²).  `%string-push` already bulk-appends a whole string's bytes
# (src/primitives/intrinsics.rs), so a string concat MUST be linear.
#
# Pre-fix this file times out: it is the `port-read-exact` /
# `port-shortread-framing` smoke hang in miniature (both build a large
# payload string by doubling concat).  Arrays/bytes (`get` is O(1)) were
# never affected — only grapheme-clustered strings.
#
# That is also what makes the bytes concat of the same size the control here:
# it runs the branch this defect cannot reach. A wall-clock bound cannot tell a
# quadratic walk from a runner that lost its CPU inside the measured window;
# the ratio between the two concats can. docs/testing.md § "A performance gate
# measures against a control" holds the argument.

# Time both thunks over `rounds` alternating rounds and return [control subject]
# — the smallest elapsed each one reached. Alternating samples the same stretch
# of machine, and the minimum discards a round the scheduler stalled, so one
# starved run cannot decide the comparison.
(defn best-of [rounds control subject]
  (let [@a nil
        @b nil
        @i 0]
    (while (< i rounds)
      (let [ca (second (time/elapsed control))
            cb (second (time/elapsed subject))]
        (when (or (nil? a) (< ca a)) (assign a ca))
        (when (or (nil? b) (< cb b)) (assign b cb)))
      (assign i (+ i 1)))
    [a b]))

# Build a >100k-grapheme string by doubling (O(log n) concats), and the
# same-length bytes value that is its control. `length` on a string counts
# graphemes, so both loops read it once per doubling and never inside a
# measurement.
(def big
  (let [@s "0123456789"]
    (while (< (length s) 100000) (assign s (concat s s)))
    s))
(def n (length big))
(def bigb
  (let [@b (bytes 0 1 2 3 4 5 6 7 8 9)]
    (while (< (length b) n) (assign b (concat b b)))
    b))

(assert (>= n 100000) "built a large string by doubling concat")
(assert (= (slice big 0 10) "0123456789") "content preserved across doublings")
(assert (= (length bigb) n) "the control value is the same size as the string")

(def doubled (concat big big))
(assert (= (length doubled) (* 2 n)) "concat length is the sum")
(assert (= (slice doubled 0 10) "0123456789") "concat content head correct")

# A single concat of the large string must complete in linear time. Post-fix it
# is a couple of byte-buffer extends, the same work the bytes concat does, so
# the multiple is slack rather than a budget. Pre-fix this one concat is
# O((1.6e5)²) — tens of seconds to minutes against tens of microseconds.
(def [control measured]
  (best-of 5 (fn () (concat bigb bigb)) (fn () (concat big big))))
(assert (< measured (* 8 control))
        (concat "string concat must be linear; (concat big big) took "
                (string measured) "s against " (string control)
                "s for the same-size bytes concat (quadratic regression)"))

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

(println "concat-linear ok: |big|=" n " concat took " measured "s against "
         control "s for bytes")
