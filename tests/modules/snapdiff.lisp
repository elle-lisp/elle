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

(defn section-drift [got want headers]
  "Where GOT and WANT first differ: a struct carrying :section (the HEADERS
   line governing that line in WANT, or \"<preamble>\" above the first one),
   :line (1-based), and :got / :want (the two lines, nil past the end of a
   shorter text). nil when the two texts are equal."
  (struct :section preamble :line 1 :got "" :want ""))

(defn drift-report [text render want headers]
  "nil when TEXT equals WANT. Otherwise a struct carrying :text (the evidence
   to keep), :kind, and the section-drift fields of the pair :kind names.
   RENDER is called a second time only on a mismatch: when it answers something
   other than TEXT the kind is :unstable and the fields read render against
   render; when it answers TEXT again the kind is :drift and they read the
   render against WANT."
  (struct :kind :drift :text text :section preamble :line 1 :got "" :want ""))

(defn drift-message [r]
  "The report as text: what it is, where it sits, and the two lines there."
  "")

(fn []
  {:section-drift section-drift
   :drift-report drift-report
   :drift-message drift-message})
