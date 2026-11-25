(elle/epoch 18)
## audited: 2026-11-22
## The binary's positional contract: one program file, then that program's argv.
## tests/cli/overview.md
## docs/config.md

(include-file "../runner/lib/harness.lisp")

# The counter-factual: the usage line read `elle [file...] [-- args...]`, and a
# caller who believed it, a Makefile passing a glob, got exit 0 with every
# file after the first unrun. The files reached the program through
# `sys/args`, and nothing advertised that reading.
(eprintln "scenario: the usage line advertises one file")
(let [r (elle-run ["--help"])]
  (assert (= r:exit 0) "--help exits 0")
  (assert (string/contains? r:out "elle [file] [args...]")
          (string "the usage line names one file and the program's args: " r:out))
  (assert (not (string/contains? r:out "[file...]"))
          (string "the usage line advertises several program files: " r:out)))

# A second `.lisp` path is an argument, not a second program: it does not run,
# and the program reads it from `sys/args`.
(with-temp-dir dir
  (eprintln "scenario: everything after the file is the program's argv")
  (let [main (write-fixture dir "main.lisp" "(print (sys/args))")
        second (write-fixture dir "second.lisp" "(print \"second ran\")")
        r (elle-run [main second "8080"])]
    (assert (= r:exit 0) (string "the run exits 0; stderr: " r:err))
    (assert (not (string/contains? r:out "second ran"))
            (string "the second file is an argument, so it must not run: " r:out))
    (assert (and (string/contains? r:out second) (string/contains? r:out "8080"))
            (string "the program reads both arguments from sys/args: " r:out))))

(eprintln "all argv scenarios passed")
