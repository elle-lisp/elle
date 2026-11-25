(elle/epoch 18)
## audited: 2026-11-22
## The binary dispatches its subcommands and its top level: lint, rewrite, --help, and a gated file.
## tests/cli/overview.md
## src/lint/AGENTS.md
## docs/test-runner.md

(include-file "../runner/lib/harness.lisp")

# A program whose one diagnostic is an error at 2:2: `g` is never bound.
(def undefined "(elle/epoch 15)\n(g)\n")

(defn exits [args code what]
  (let [r (elle-run args)]
    (assert (= r:exit code) (string what ": exit " r:exit "; stderr: " r:err))
    r))

(eprintln "scenario: lint answers its help and lints the fixtures")
(exits ["lint" "--help"] 0 "elle lint --help")
(exits ["lint" "tests/fixtures/naming-good.lisp"] 0 "lint of a clean file")
# The linter checks no naming convention, so naming-bad.lisp lints clean.
(exits ["lint" "tests/fixtures/naming-bad.lisp"] 0 "lint of naming-bad.lisp")
(let [r (elle-run ["lint" "--format" "json" "tests/fixtures/naming-bad.lisp"])]
  (assert (string/contains? r:out "\"diagnostics\"")
          (string "the JSON output holds diagnostics: " r:out)))
(let [r (elle-run ["lint" "nonexistent-file-that-does-not-exist.lisp"])]
  (assert (not (= r:exit 0)) "lint of a missing file exits nonzero"))

# The counter-factual: the help said `warning (default)` while the linter
# started from `info`.
(eprintln "scenario: lint's help names info as the default level")
(let [r (exits ["lint" "--help"] 0 "elle lint --help")
      level (filter (fn [l] (string/contains? l "--level")) (lines r:out))]
  (assert (not (empty? level)) (string "no --level line: " r:out))
  (assert (string/contains? (first level) "info (default)")
          (string "got " (first level)))
  (assert (= (length (string/split (first level) "(default)")) 2)
          (string "one level is the default: " (first level))))

(with-temp-dir dir
  # The counter-factual: a line read `2:2 error: …`, and over several files the
  # reader could not tell which file it was about.
  (eprintln "scenario: lint's text names the file of each diagnostic")
  (let [a (write-fixture dir "a.lisp" undefined)
        r (exits ["lint" a] 1 "lint of an undefined name")]
    (assert (string/contains? r:out (string a ":2:2 error: undefined-variable"))
            (string "got " r:out)))

  (eprintln "scenario: lint's JSON names the file of each diagnostic")
  (let [a (path/join dir "a.lisp")
        r (exits ["lint" "--format" "json" a] 1 "lint --format json")
        diagnostics (get (json/parse r:out) "diagnostics")]
    (assert (= (length diagnostics) 1) (string "got " r:out))
    (assert (= (get (first diagnostics) "file") a) (string "got " r:out)))

  # Eight names, written in reverse: a listing that happened to come back
  # sorted would pass for the wrong reason.
  (eprintln "scenario: lint of a directory prints its files in sorted order")
  (let [root (path/join dir "tree")
        names ["a" "b" "c" "d" "e" "f" "g" "h"]]
    (file/mkdir-all root)
    (each n (reverse names)
      (write-fixture root (string n ".lisp") undefined))
    (let [r (exits ["lint" root] 1 "lint of a directory")
          at (map (fn [n]
                    (string/find r:out
                                 (string (path/join root (string n ".lisp"))
                                   ":2:2"))) names)]
      (assert (all? (fn [i] (not (nil? i))) at)
              (string "a file is missing: " r:out))
      (assert (= at (sort at)) (string "the files print out of order: " r:out)))))

(eprintln "scenario: a file runs as it always did")
(exits ["docs/syntax.md"] 0 "a literate file runs")

(eprintln "scenario: the help mentions lint and lsp")
(let [r (elle-run ["--help"])]
  (assert (and (string/contains? r:out "lint") (string/contains? r:out "lsp"))
          (string "--help names lint and lsp: " r:out)))

# Run directly, an unmet gate is a clean skip: exit 0 with the reason on
# stderr, so `gate!` is the one skip, in place of `(sys/exit 0)`. The
# counter-factual: an uncaught :gated exited nonzero like any other error.
(eprintln "scenario: an unmet gate at top level exits 0 with a skip")
(let [r (exits ["tests/fixtures/gated-toplevel.lisp"] 0 "an unmet gate")]
  (assert (and (string/contains? r:err "SKIP")
               (string/contains? r:err "dependency absent"))
          (string "the gate's reason is reported as a skip: " r:err))
  (assert (not (string/contains? r:out "must print"))
          (string "the gated body and the form after it must not run: " r:out)))

# Only :gated is a clean skip, or a real failure would read as a skip.
(eprintln "scenario: an ordinary error at top level still exits nonzero")
(let [r (elle-run ["-e" "(error \"boom\")"])]
  (assert (not (= r:exit 0)) "an uncaught error exits nonzero"))

(eprintln "scenario: rewrite answers its help")
(let [r (exits ["rewrite" "--help"] 0 "elle rewrite --help")]
  (assert (string/contains? r:out "elle rewrite") (string "got " r:out)))

(eprintln "all dispatch scenarios passed")
