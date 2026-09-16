(elle/epoch 12)
# audited: 2026-09-16
# tests/elle/snapdiff.lisp — the drift report a sectioned golden fails with.
#
# tests/modules/snapdiff.lisp turns "the two texts differ" into a reading of the
# difference, which is the whole reason the escape golden
# (tests/elle/escape-golden.lisp) can be diagnosed from one recorded line. These
# pin what that reading says.
#
# THE TRAP. `drift-report` renders a SECOND time on a mismatch, and a golden
# whose render leaks (compile/dumps does) pays for every extra call. The
# clean-path case below counts the calls, because "renders once when the texts
# match" is the property that keeps the cost where it was.

(def snapdiff ((import-file "tests/modules/snapdiff.lisp")))

(def headers ["[alpha]" "[beta]"])

(def want (string "head\n[alpha]\n  one\n  two\n[beta]\n  three\n"))

# ── section-drift ────────────────────────────────────────────────────

(assert (= (snapdiff:section-drift want want headers) nil)
        "equal texts are no drift")

# A changed line inside the last section names that section, and the line
# number counts the whole text, not the section.
(let [got (string "head\n[alpha]\n  one\n  two\n[beta]\n  THREE\n")
      d (snapdiff:section-drift got want headers)]
  (assert (= (get d :section) "[beta]") (string "section was " (get d :section)))
  (assert (= (get d :line) 6) (string "line was " (get d :line)))
  (assert (= (get d :got) "  THREE") (string "got was " (get d :got)))
  (assert (= (get d :want) "  three") (string "want was " (get d :want))))

# A changed line in an earlier section names that one — the counter-factual for
# the escape golden, where a drift above [region_instrs] is an escape change and
# one inside it is only region placement.
(let [got (string "head\n[alpha]\n  ONE\n  two\n[beta]\n  three\n")
      d (snapdiff:section-drift got want headers)]
  (assert (= (get d :section) "[alpha]")
          (string "section was " (get d :section)))
  (assert (= (get d :line) 3) (string "line was " (get d :line))))

# Above the first header there is no section to name.
(let [got (string "HEAD\n[alpha]\n  one\n  two\n[beta]\n  three\n")
      d (snapdiff:section-drift got want headers)]
  (assert (= (get d :section) "<preamble>")
          (string "section was " (get d :section)))
  (assert (= (get d :line) 1) (string "line was " (get d :line))))

# A header line that moves belongs to the section it opens, not to the one
# above it.
(let [got (string "head\n[alpha]\n  one\n  two\n[gamma]\n  three\n")
      d (snapdiff:section-drift got want headers)]
  (assert (= (get d :section) "[beta]") (string "section was " (get d :section)))
  (assert (= (get d :got) "[gamma]") (string "got was " (get d :got))))

# THE TRAP. A text ending in a newline splits to a final EMPTY line, so an
# extra line at the end reads against "" rather than against nothing. Both
# snapshot and render end in a newline, which is the case the golden meets.
(let [got (string "head\n[alpha]\n  one\n  two\n[beta]\n  three\n  four\n")
      d (snapdiff:section-drift got want headers)]
  (assert (= (get d :line) 7) (string "line was " (get d :line)))
  (assert (= (get d :got) "  four") (string "got was " (get d :got)))
  (assert (= (get d :want) "") (string "want was " (get d :want))))

# Past the end of the shorter text there is no line at all, and nil says so.
(let [d (snapdiff:section-drift "[alpha]\n  one\n  two" "[alpha]\n  one" headers)]
  (assert (= (get d :line) 3) (string "line was " (get d :line)))
  (assert (= (get d :got) "  two") (string "got was " (get d :got)))
  (assert (= (get d :want) nil) (string "want was " (get d :want))))

# ── drift-report ─────────────────────────────────────────────────────

# A matching text is no report, and the renderer is never called again.
(let [calls @[]
      render (fn []
               (push calls 1)
               want)]
  (assert (= (snapdiff:drift-report want render want headers) nil)
          "a matching text is no report")
  (assert (= (length calls) 0)
          (string "render was called " (length calls) " times on the clean path")))

# Two renders that agree make the mismatch a snapshot drift, read against the
# snapshot.
(let [got (string "head\n[alpha]\n  one\n  two\n[beta]\n  THREE\n")
      r (snapdiff:drift-report got (fn [] got) want headers)]
  (assert (= (get r :kind) :drift) (string "kind was " (get r :kind)))
  (assert (= (get r :text) got) "the report carries the rendered text")
  (assert (= (get r :section) "[beta]") (string "section was " (get r :section)))
  (assert (= (get r :want) "  three") (string "want was " (get r :want))))

# Two renders that disagree make it an unstable renderer, read render against
# render — the snapshot is not what moved.
(let [first (string "head\n[alpha]\n  one\n  two\n[beta]\n  THREE\n")
      second (string "head\n[alpha]\n  one\n  TWO\n[beta]\n  THREE\n")
      r (snapdiff:drift-report first (fn [] second) want headers)]
  (assert (= (get r :kind) :unstable) (string "kind was " (get r :kind)))
  (assert (= (get r :text) first) "the report carries the first render")
  (assert (= (get r :section) "[alpha]")
          (string "section was " (get r :section)))
  (assert (= (get r :line) 4) (string "line was " (get r :line))))

# ── drift-message ────────────────────────────────────────────────────

(let [got (string "head\n[alpha]\n  one\n  two\n[beta]\n  THREE\n")
      m (snapdiff:drift-message (snapdiff:drift-report got (fn [] got) want
                                headers))]
  (assert (string/contains? m "snapshot drift") (string "message was " m))
  (assert (string/contains? m "[beta]") (string "message was " m))
  (assert (string/contains? m "line 6") (string "message was " m))
  (assert (string/contains? m "  THREE") (string "message was " m)))

(let [first (string "head\n[alpha]\n  one\n  two\n[beta]\n  THREE\n")
      second (string "head\n[alpha]\n  one\n  TWO\n[beta]\n  THREE\n")
      m (snapdiff:drift-message (snapdiff:drift-report first (fn [] second) want
                                headers))]
  (assert (string/contains? m "two renders of one source disagree")
          (string "message was " m)))

(println "snapdiff: drift reports read by section")
