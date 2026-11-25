(elle/epoch 18)
## audited: 2026-11-22
## A file's name selects no syntax: the binary runs a file of any name as Elle, and its help names .lisp and .md alone.
## tests/cli/overview.md
## docs/impl/reader.md

(include-file "../runner/lib/harness.lisp")

# The counter-factual: the help listed `.py`, `.js` and `.lua` as syntaxes the
# binary reads, after nothing read them any more.
(eprintln "scenario: the help names s-expressions and markdown as the only syntaxes")
(let [r (elle-run ["--help"])]
  (assert (and (string/contains? r:out ".lisp") (string/contains? r:out ".md"))
          (string "--help names .lisp and .md: " r:out))
  (each other [".py" ".js" ".lua"]
    (assert (not (string/contains? r:out other))
            (string "--help names " other ": " r:out))))

# The counter-factual: a `.py`, `.js` or `.lua` file went to a reader for that
# language, which refused this program or read it as something else.
(with-temp-dir dir
  (eprintln "scenario: a file named for another language runs as Elle")
  (each name ["prog.py" "prog.js" "prog.lua"]
    (let [r (elle-run [(write-fixture dir name "(println (+ 1 2))\n")])]
      (assert (= r:exit 0) (string name ": stderr: " r:err))
      (assert (= r:out "3\n") (string name ": printed " r:out)))))

(eprintln "all source-names scenarios passed")
