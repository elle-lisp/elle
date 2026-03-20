#!/usr/bin/env elle
## Rename (elle N) to (elle/epoch N) in all .lisp files.

(def glob-plugin (import "target/release/libelle_glob.so"))
(def do-glob (get glob-plugin :glob))

(def files (append
  (append
    (do-glob "tests/**/*.lisp")
    (do-glob "examples/**/*.lisp"))
  (do-glob "scripts/**/*.lisp")))

(var count 0)

(each f in files
  (when (not (string/contains? f "AGENTS"))
    (def content (slurp f))
    (when (string/starts-with? content "(elle ")
      (def new-content (string/replace content "(elle " "(elle/epoch "))
      (spit f new-content)
      (assign count (+ count 1)))))

(print (string count " files updated"))
