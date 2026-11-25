(elle/epoch 18)
## audited: 2026-11-22
## A partial commit takes a Lisp file the hook leaves alone, and refuses one it changed, keeping the real index at HEAD.
## tests/cli/overview.md
## CONTRIBUTING.md

(include-file "../runner/lib/harness.lisp")
(include-file "lib/pre-commit.lisp")

(with-temp-dir dir
  # The counter-factual: a hook that refuses every partial commit passes the
  # scenarios below and makes `git commit -- PATHS` unusable.
  (eprintln "scenario: a partial commit of a formatted file goes through")
  (let [r (scratch-repo dir "partial-tidy")
        change (formatted other)]
    (repo-seed r "f.lisp" (formatted tidy))
    (repo-write r "f.lisp" change)
    (let [out (repo-commit r ["--" "f.lisp"])]
      (assert (= out:exit 0) (string "the commit failed: " (transcript out))))
    (assert (= (repo-head r "f.lisp") change) "HEAD holds the change")
    (at-head r))

  (eprintln "scenario: a partial commit the hook reformats is refused and formatted on disk")
  (let [r (scratch-repo dir "partial-refused")]
    (repo-seed r "f.lisp" (formatted tidy))
    (let [before (must r ["rev-parse" "HEAD"])]
      (repo-write r "f.lisp" untidy)
      (let [out (repo-commit r ["--" "f.lisp"])]
        (assert (not (= out:exit 0))
                "the hook must refuse a partial commit it reformatted")
        (assert (string/contains? (transcript out) "f.lisp")
                (string "the refusal names the file: " (transcript out))))
      (assert (= (must r ["rev-parse" "HEAD"]) before) "nothing committed"))
    (assert (= (repo-read r "f.lisp") (formatted untidy))
            "the hook formats the file on disk")
    (at-head r))

  # Running the same commit again takes the file the hook formatted, and the
  # real index agrees with what landed. The seed differs from the formatted
  # change, or the second attempt has nothing to commit.
  (eprintln "scenario: a partial commit never leaves the real index behind HEAD")
  (let [r (scratch-repo dir "partial-again")]
    (repo-seed r "f.lisp" (formatted other))
    (repo-write r "f.lisp" untidy)
    (when (= 0 (get (repo-commit r ["--" "f.lisp"]) :exit)) (at-head r))
    (let [out (repo-commit r ["--" "f.lisp"])]
      (assert (= out:exit 0)
              (string "the second attempt commits the formatted file: "
                      (transcript out))))
    (assert (= (repo-head r "f.lisp") (formatted untidy))
            "HEAD holds the file formatted")
    (assert (= (repo-read r "f.lisp") (repo-head r "f.lisp"))
            "and so does the disk")
    (at-head r))

  (eprintln "scenario: a partial commit the hook migrates is refused")
  (let [r (scratch-repo dir "partial-migrate")]
    (repo-seed r "f.lisp" (formatted tidy))
    (repo-write r "f.lisp" old-epoch)
    (assert (not (= 0 (get (repo-commit r ["--" "f.lisp"]) :exit)))
            "the hook must refuse a partial commit it migrated")
    (assert (= (repo-read r "f.lisp") (formatted old-epoch))
            "the hook migrates the file on disk")
    (at-head r))

  # The generator rewrites an AGENTS.md the commit does not name. Staging it
  # into the temporary index would commit it past the real index, so the hook
  # refuses until the author names it too.
  (eprintln "scenario: a partial commit refuses an index the generator rewrote")
  (let [r (scratch-repo dir "partial-agents")]
    (repo-seed r "AGENTS.md" "old\n")
    (repo-seed r "f.lisp" (formatted tidy))
    (repo-write r "AGENTS.next" "new\n")
    (repo-write r "f.lisp" (formatted other))
    (let [out (repo-commit r ["--" "f.lisp"])]
      (assert (not (= out:exit 0))
              "the hook must refuse a partial commit that leaves the regenerated index out")
      (assert (string/contains? (transcript out) "AGENTS.md")
              (string "the refusal names the index: " (transcript out))))
    (at-head r)
    (let [out (repo-commit r ["--" "f.lisp" "AGENTS.md"])]
      (assert (= out:exit 0)
              (string "naming the index commits it: " (transcript out))))
    (assert (= (repo-head r "AGENTS.md") "new\n")
            "HEAD holds the regenerated index")
    (at-head r)))

(eprintln "all pre-commit-partial scenarios passed")
