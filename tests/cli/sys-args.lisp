(elle/epoch 18)
## audited: 2026-11-22
## What reaches the running program: `sys/args` and `sys/argv`, end to end.
## tests/cli/overview.md
## docs/config.md
##
## Every argument after the source file, or the stdin `-`, belongs to the
## program, a `--` included, and no separator is needed to say so. `main` is
## what fills the program's arguments, so these start the binary.

(include-file "../runner/lib/harness.lisp")

(defn prints [args source want what]
  "Run SOURCE on stdin under ARGS, and require exit 0 and WANT on stdout."
  (let [r (elle-feed args source)]
    (assert (= r:exit 0) (string what "; stderr: " r:err))
    (assert (= (string/trim r:out) want)
            (string what ": printed " r:out ", wanted " want))))

# A flag named like one of elle's own still belongs to the program once `--`
# has ended elle's flags. The counter-factual: a `main` that scans the whole
# argv for `--help` and `--version` answers these with the banner, and the
# program never runs.
(eprintln "scenario: --help and --version after the separator belong to the program")
(prints ["-" "--" "--help"] "(print (sys/args))" "(-- --help)" "--help after --")
(prints ["-" "--" "--version"] "(print (sys/args))" "(-- --version)"
        "--version after --")

(eprintln "scenario: --help and --version before the separator are elle's own")
(let [version (elle-run ["--version"])
      help (elle-run ["--help"])]
  (assert (string/starts-with? (string/trim version:out) "Elle v")
          (string "--version before any source is elle's: " version:out))
  (assert (string/contains? help:out "Usage: elle")
          (string "--help before any source is elle's: " help:out)))

# `--` ends elle's flags for every flag, which is the rule `--help` and
# `--version` join rather than a special case written for them.
(eprintln "scenario: an ordinary flag after the separator reaches the program untouched")
(prints ["-" "--" "--trace=call"]
        "(print (sys/args)) (println \" trace=\" (vm/config :trace))"
        "(-- --trace=call) trace=||"
        "the program gets the flag and tracing stays off")

(eprintln "scenario: sys/args is empty with no trailing arguments")
(prints ["-"] "(print (sys/args))" "()" "an empty list, never nil")

(eprintln "scenario: sys/args holds every trailing argument")
(prints ["-" "foo" "bar"] "(print (sys/args))" "(foo bar)" "two arguments")

(eprintln "scenario: a flag after the source is a program argument")
(prints ["-" "-v" "foo"] "(print (sys/args))" "(-v foo)" "-v after the source")

(eprintln "scenario: sys/argv puts the source name first")
(prints ["-" "foo" "bar"] "(print (sys/argv))" "(- foo bar)" "stdin and two")
(prints ["-"] "(print (sys/argv))" "(-)" "the source name alone")
(prints ["-" "-v" "foo"] "(print (sys/argv))" "(- -v foo)" "a flag after it")

(eprintln "all sys-args scenarios passed")
