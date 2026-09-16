(elle/epoch 12)
# audited: 2026-09-16
# tests/modules/snapdiff.lisp — where two snapshot texts first differ, which
# section of the reference that line sits in, and whether the renderer agrees
# with itself.
#
# A golden that pins a sectioned text byte-for-byte can say only "drift". The
# reader is left with a verdict and no reading of it, and both texts are gone
# when the process exits. This turns the comparison into a report: the first
# differing line, the section governing it, and the two lines themselves.
#
# It also asks the question a byte-compare cannot. On a mismatch it renders a
# second time. Two renders that disagree name an unstable renderer, which is a
# different defect from a snapshot that is out of date, and the report says
# which one the reader has.

(def preamble "<preamble>")

(defn split-lines [text]
  (string/split text "\n"))

(defn line-at [lines i]
  (if (< i (length lines)) (get lines i) nil))

(defn header? [headers line]
  (any? (fn [h] (= h line)) headers))

(defn section-drift [got want headers]
  "Where GOT and WANT first differ: a struct carrying :section (the HEADERS
   line governing that line in WANT, or \"<preamble>\" above the first one),
   :line (1-based), and :got / :want (the two lines, nil past the end of a
   shorter text). nil when the two texts are equal."
  (if (= got want)
    nil
    (let [gl (split-lines got)
          wl (split-lines want)
          n (max (length gl) (length wl))]
      (var section preamble)
      (var i 0)
      (var out nil)
      (while (and (= out nil) (< i n))
        (let [g (line-at gl i)
              w (line-at wl i)]
          (when (and (not (= w nil)) (header? headers w)) (assign section w))
          (when (not (= g w))
            (assign out (struct :section section :line (+ i 1) :got g :want w)))
          (assign i (+ i 1))))
      out)))

(defn report-of [kind text d]
  (struct :kind kind :text text :section (get d :section) :line (get d :line)
          :got (get d :got) :want (get d :want)))

(defn drift-report [text render want headers]
  "nil when TEXT equals WANT. Otherwise a struct carrying :text (the evidence
   to keep), :kind, and the section-drift fields of the pair :kind names.
   RENDER is called a second time only on a mismatch: when it answers something
   other than TEXT the kind is :unstable and the fields read render against
   render; when it answers TEXT again the kind is :drift and they read the
   render against WANT."
  (if (= text want)
    nil
    (let [again (render)]
      (if (= again text)
        (report-of :drift text (section-drift text want headers))
        (report-of :unstable text (section-drift again text headers))))))

(defn drift-message [r]
  "The report as text: what it is, where it sits, and the two lines there."
  (string (if (= (get r :kind) :unstable)
            "two renders of one source disagree"
            "snapshot drift") " in " (get r :section) " at line " (get r :line)
          "\n    got:  " (string (get r :got)) "\n    want: "
          (string (get r :want))))

(fn []
  {:section-drift section-drift
   :drift-report drift-report
   :drift-message drift-message})
