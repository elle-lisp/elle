(elle/epoch 18)
## audited: 2026-11-22
## `meta/origin` answers a closure's file, line and column through the whole pipeline, and nil for anything else.
## tests/cli/overview.md
## docs/analysis/debugging.md

(include-file "../runner/lib/harness.lisp")

(with-temp-dir dir
  (eprintln "scenario: meta/origin answers the file and line of a defn")
  (let [path (write-fixture dir "origin.lisp"
                            "(defn foo () 42)\n(print (meta/origin foo))\n")
        r (elle-run [path])]
    (assert (= r:exit 0) (string "elle failed: " r:err))
    (assert (string/contains? r:out path)
            (string "the origin names the file " path ": " r:out))
    (assert (and (string/contains? r:out ":line")
                 (string/contains? r:out ":col"))
            (string "the origin carries a line and a column: " r:out))))

(eprintln "scenario: meta/origin of a value that is no closure is nil")
(let [r (elle-feed ["-"] "(print (meta/origin 42))")]
  (assert (= r:exit 0) (string "elle failed: " r:err))
  (assert (= (string/trim r:out) "nil") (string "printed " r:out)))

(eprintln "all meta-origin scenarios passed")
