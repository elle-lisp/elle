(elle/epoch 18)
## audited: 2026-11-22
## A file that calls `exit` under the runner ends its own run, not the whole run.
## tests/runner/overview.md
## docs/test-runner.md
##
## `exit` lowers to `std::process::exit`, which nothing catches. The runner sets
## a per-thread exit trap around each file, so `exit` raises
## `{:error :exited :code N}` instead: a skip for code 0, a fail otherwise.
##
## The counter-factual: without the trap, the first file's `(exit 0)` ends the
## runner with code 0, the second file never runs, and the gate reads green. The
## second file fails an assertion, so a run that reached it gates 1.

(include-file "lib/harness.lisp")

(with-temp-dir dir
  (eprintln "scenario: calling exit does not truncate the run")
  (let [r (elle-test (path/join dir "s.db")
                     [(write-fixture dir "a-exits.lisp" "(exit 0)\n")
                      (write-fixture dir "b-fails.lisp"
                                     "(assert false \"second file was reached\")\n")])]
    (assert (= r:exit 1)
            (string "a leading (exit 0) truncated the run, or killed it: the "
                    "second file fails, so the gate must exit 1, got " r:exit
                    " — stderr: " r:err)))

  (eprintln "all exit-trap scenarios passed"))
