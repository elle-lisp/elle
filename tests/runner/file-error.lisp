(elle/epoch 18)
## audited: 2026-11-22
## A path the runner cannot read, and a source that does not parse, each become one file-level fail row, and the run goes on.
## tests/runner/overview.md
## docs/test-runner.md
##
## The counter-factual: the runner reads a file before it compiles it, and a
## read that raises outside `protect` ends the whole run. The run row then says
## DID NOT COMPLETE, the paths after the bad one never run, and the error names
## the runner's own source line rather than the file at fault.

(include-file "lib/harness.lisp")

(with-temp-dir dir
  (def passes
    (write-fixture dir "passes.lisp" "(assert true \"the next path runs\")\n"))

  (defn one-file-level-fail [name args bad said]
    "Run ARGS into a store of its own, and require the shape every case
     shares: the run finished with one fail, the source named BAD is one
     file-level `vm` row whose reason carries SAID, and passes.lisp passed."
    (let [db (path/join dir (string name ".db"))
          r (elle-test db args)
          run (get (rows db
                         (string "SELECT (finished_at IS NOT NULL) AS done, "
                                 "n_fail AS fails FROM run WHERE id = "
                                 latest-run)) 0)
          res (rows db
                    (string "SELECT f.file AS file, f.form_index AS idx, "
                            "r.tier AS tier, r.status AS status, "
                            "r.reason AS reason FROM result r "
                            "JOIN form f ON f.hash = r.form_hash "
                            "WHERE r.run_id = " latest-run))
          bad-rows (filter (fn [row] (string/contains? row:file bad)) res)
          next-rows (filter (fn [row] (string/contains? row:file "passes.lisp"))
                            res)]
      (assert (not (= r:exit 0))
              (string name ": a file-level failure gates the run; stderr: "
                      r:err))
      (assert (= run:done 1)
              (string name ": the run must reach its end; stderr: " r:err))
      (assert (= run:fails 1)
              (string name ": the bad source is one fail, got " run:fails))
      (assert (= (length bad-rows) 1)
              (string name ": one row names " bad ", got " res))
      (let [row (get bad-rows 0)]
        (assert (and (= row:idx -1) (= row:tier "vm") (= row:status "fail"))
                (string name ": the bad source is one file-level vm fail, got "
                        row))
        (assert (string/contains? row:reason said)
                (string name ": the reason must carry " said ", got " row:reason)))
      (assert (and (not (empty? next-rows))
                   (all? (fn [row] (= row:status "pass")) next-rows))
              (string name
                      ": the path after the bad one must run and pass, got "
                      next-rows))))

  (eprintln "scenario: a path that cannot be read is one file-level failure")
  (one-file-level-fail "missing" [(path/join dir "missing.lisp") passes]
                       "missing.lisp" "No such file")

  (eprintln "scenario: a file that does not parse is one file-level failure")
  (one-file-level-fail "unclosed"
                       [(write-fixture dir "unclosed.lisp"
                                       "(assert true \"unclosed\"\n") passes]
                       "unclosed.lisp" "unterminated list")

  # An `-e` form has no path, so the run's order is the paths, then the forms.
  # The passing path runs first here, and the bad form is the run's last word.
  (eprintln "scenario: an eval that does not parse is one file-level failure")
  (one-file-level-fail "eval" [passes "-e" "(+ 1"] "<eval>" "unterminated list")

  (eprintln "all file-error scenarios passed"))
