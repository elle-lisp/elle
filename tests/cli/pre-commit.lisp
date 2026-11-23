(elle/epoch 18)
## audited: 2026-11-22
## A plain commit takes the staged content of each Lisp file formatted, and leaves the real index at HEAD.
## tests/cli/overview.md
## CONTRIBUTING.md

(include-file "../runner/lib/harness.lisp")
(include-file "lib/pre-commit.lisp")

(with-temp-dir dir
  (eprintln "scenario: a plain commit stages the reformatted file")
  (let [r (scratch-repo dir "plain")]
    (repo-write r "f.lisp" untidy)
    (must r ["add" "f.lisp"])
    (let [out (repo-commit r [])]
      (assert (= out:exit 0) (string "the commit failed: " (transcript out))))
    (assert (= (repo-head r "f.lisp") (formatted untidy))
            "HEAD holds the file formatted")
    (assert (not (= (repo-head r "f.lisp") untidy))
            "the fixture must need formatting")
    (assert (= (repo-read r "f.lisp") (repo-head r "f.lisp"))
            "and so does the disk")
    (at-head r))

  # `git commit -a` also builds its own index, but that index becomes the real
  # one, so the hook may stage into it and the commit goes through.
  (eprintln "scenario: commit -a stages the reformatted file")
  (let [r (scratch-repo dir "all")]
    (repo-seed r "f.lisp" (formatted tidy))
    (repo-write r "f.lisp" untidy)
    (let [out (repo-commit r ["-a"])]
      (assert (= out:exit 0) (string "the commit failed: " (transcript out))))
    (assert (= (repo-head r "f.lisp") (formatted untidy))
            "HEAD holds the file formatted")
    (at-head r))

  (eprintln "scenario: a plain commit migrates an older epoch")
  (let [r (scratch-repo dir "migrate")]
    (repo-write r "f.lisp" old-epoch)
    (must r ["add" "f.lisp"])
    (let [out (repo-commit r [])]
      (assert (= out:exit 0) (string "the commit failed: " (transcript out))))
    (let [head (repo-head r "f.lisp")]
      (assert (and (string/contains? head "(pair 1 2)")
                   (not (string/contains? head "elle/epoch 9")))
              (string "the hook migrates the file: " head))
      (assert (= head (formatted old-epoch)) (string "HEAD holds " head)))
    (at-head r))

  # The counter-factual: format the working copy and `git add` it, and the
  # commit sweeps in every hunk the author left unstaged.
  (eprintln "scenario: a plain commit leaves an unstaged change unstaged")
  (let [r (scratch-repo dir "unstaged")
        working (string untidy "(def extra   1)\n")]
    (repo-seed r "f.lisp" (formatted tidy))
    (repo-write r "f.lisp" untidy)
    (must r ["add" "f.lisp"])
    (repo-write r "f.lisp" working)
    (let [out (repo-commit r [])]
      (assert (= out:exit 0) (string "the commit failed: " (transcript out))))
    (assert (= (repo-head r "f.lisp") (formatted untidy))
            "the commit takes the staged content, formatted, and nothing else")
    (at-head r)
    (assert (= (repo-read r "f.lisp") (formatted working))
            "the working copy keeps its unstaged change, formatted"))

  # The counter-factual: `elle fmt f && git add f` skips the file on a failure,
  # and the commit lands it exactly as the author left it.
  (eprintln "scenario: a file elle fmt cannot read is refused")
  (let [r (scratch-repo dir "unreadable")]
    (repo-write r "f.lisp" "(defn f [x]\n  (+ x 1)\n")
    (must r ["add" "f.lisp"])
    (let [out (repo-commit r [])]
      (assert (not (= out:exit 0))
              "the hook must refuse a file its formatter cannot read")
      (assert (string/contains? (transcript out) "f.lisp")
              (string "the refusal names the file: " (transcript out))))
    (assert (not (= 0 (get (git r ["cat-file" "-e" "HEAD:f.lisp"]) :exit)))
            "nothing committed")))

(eprintln "all pre-commit scenarios passed")
