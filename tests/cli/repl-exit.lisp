(elle/epoch 18)
## audited: 2026-11-22
## The REPL exits 1 when piped input fails to parse, and 0 when all of it runs.
## tests/cli/overview.md
## docs/config.md

(include-file "../runner/lib/harness.lisp")

(eprintln "scenario: piped input that fails to parse exits 1")
(let [r (elle-feed [] "(+ 1 1\n")]
  (assert (= r:exit 1) (string "exit " r:exit "; stderr: " r:err))
  (assert (string/contains? r:err "unterminated")
          (string "the parse error is reported: " r:err)))

(eprintln "scenario: piped input that runs exits 0")
(let [r (elle-feed [] "(+ 1 1)\n")]
  (assert (= r:exit 0) (string "exit " r:exit "; stderr: " r:err)))

(eprintln "scenario: one bad expression after a good one exits 1")
(let [r (elle-feed [] "(+ 1 1)\n(* 2 2\n")]
  (assert (= r:exit 1) (string "exit " r:exit "; stderr: " r:err)))

(eprintln "all repl-exit scenarios passed")
