(elle/epoch 18)
## audited: 2026-11-22
## What a file a REPL line imports cannot see: the macros and the definitions earlier lines made.
## tests/cli/overview.md
## docs/pipeline.md

(include-file "../runner/lib/harness.lisp")
(include-file "lib/repl.lisp")

(with-temp-dir dir
  (defn imports-in [name module input]
    "Write MODULE as m.lisp in a directory of its own, and feed INPUT to a
     REPL started there."
    (let [home (path/join dir name)]
      (file/mkdir-all home)
      (write-fixture home "m.lisp" module)
      (session input home)))

  # The file expands against the prelude and its own macros, as from any
  # program. The counter-factual handed the REPL macros to the expander that
  # `eval` builds, so the import expanded rmac and answered 43.
  (eprintln "scenario: a REPL macro does not reach an imported file")
  (let [r (imports-in "macro" "(+ (rmac) 1)\n"
                      "(defmacro rmac [] 42)\n(rmac)\n(import/load-file \"m.lisp\")\n")]
    (assert (echoed? r "42") (string "a later line expands rmac: " r:out))
    (assert (not (echoed? r "43")) (string "the import expanded rmac: " r:out))
    (assert (string/contains? r:err "undefined variable: rmac")
            (string "the import compiles without the REPL macro: " r:err)))

  # The counter-factual registered REPL bindings with the instance, so the
  # import resolved rdef and answered 43.
  (eprintln "scenario: a REPL def does not reach an imported file")
  (let [r (imports-in "def" "(+ rdef 2)\n"
                      "(def rdef 41)\n(+ rdef 1)\n(import/load-file \"m.lisp\")\n")]
    (assert (echoed? r "42") (string "a later line resolves rdef: " r:out))
    (assert (not (echoed? r "43")) (string "the import resolved rdef: " r:out))
    (assert (string/contains? r:err "undefined variable: rdef")
            (string "the import compiles without the REPL binding: " r:err))))

(eprintln "all repl-imports scenarios passed")
