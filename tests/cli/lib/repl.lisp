(elle/epoch 18)
## audited: 2026-11-22
## A REPL session fed through a pipe, and what each of its lines echoed.
## tests/cli/overview.md
## docs/pipeline.md
##
## Splice this after the runner's harness, whose elle-feed it reads. The REPL
## echoes each value it evaluates as `⟹ VALUE` on stdout.

(defn session [input &opt cwd]
  "Feed INPUT to a REPL started in CWD, and answer what the session left."
  (elle-feed [] input :cwd cwd))

(defn echoed? [r value]
  "Whether session R echoed VALUE at the end of a line: a side effect that
   printed no newline shares the line before an echo."
  (any? (fn [l] (string/ends-with? (string/trim l) (string "⟹ " value)))
        (lines r:out)))

(defn echoes [input values what]
  "Run INPUT as a session that must exit 0 and echo each of VALUES."
  (let [r (session input)]
    (assert (= r:exit 0) (string what ": exit " r:exit "; stderr: " r:err))
    (each v values
      (assert (echoed? r v) (string what ": no echo of " v " in " r:out)))
    r))
