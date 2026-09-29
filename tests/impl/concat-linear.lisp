(elle/epoch 12)
# audited: 2026-09-29
# A string concat takes time linear in its length, measured against a bytes concat of the same size.
# docs/testing.md
#
# `push-all` (src/core.lisp) bulk-appends a string source with one
# `%string-push` (src/primitives/intrinsics/data.rs), which copies the whole
# string's bytes. The counter-factual is a `push-all` that walks a string with
# `(get src i)`: `get` on a string is `segment::graphemes(s, gen).nth(i)`,
# which is O(i), so `(concat s s)` over an L-grapheme string is O(L²). A test
# that builds a large payload by doubling concat then runs for minutes.
#
# A bytes concat of the same size is the control, because `get` on bytes is
# O(1). A wall-clock bound cannot tell a quadratic walk from a runner that lost
# its CPU inside the measured window; the ratio between the two concats can.
# docs/testing.md holds the argument.

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

# A single concat of the large string must complete in linear time. It is a
# couple of byte-buffer extends, the same work the bytes concat does, so the
# multiple is slack rather than a budget. A per-grapheme walk makes this one
# concat O((1.6e5)²): tens of seconds to minutes against tens of microseconds.
(def [control measured]
  (best-of 5 (fn () (concat bigb bigb)) (fn () (concat big big))))
(assert (< measured (* 8 control))
        (concat "string concat must be linear; (concat big big) took "
                (string measured) "s against " (string control)
                "s for the same-size bytes concat (quadratic regression)"))

# The linear path bulk-appends a whole string via %string-push, which must
# accept BOTH an immutable string and a mutable @string as the pushed value
# (push-all / concat feed it the source collection directly). Exercise the
# @string-source cases, which a per-grapheme walk never reaches.
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
