#!/usr/bin/env elle
(elle/epoch 8)
## Add (elle N) declaration to all .lisp files that don't already have one.
## Usage: elle tools/add-epoch.lisp -- <epoch>

(def args (drop 1 (sys/args)))
(when (empty? args)
  (print "Usage: elle tools/add-epoch.lisp -- <epoch>")
  (exit 1))

(def epoch (parse-int (first args)))

(def glob-plugin (import "plugin/glob"))
(def do-glob (get glob-plugin :glob))

(def files (append
  (do-glob "tests/**/*.lisp")
  (do-glob "examples/**/*.lisp")))

(def tag (string "(elle " (string epoch) ")"))
(def @count 0)

(each f in files
  (when (not (string/contains? f "AGENTS"))
    (def content (slurp f))
    (unless (string/starts-with? content "(elle ")
      (spit f (string tag "\n" content))
      (print (string "  " f))
      (assign count (+ count 1)))))

(print (string count " files updated with " tag))
