#!/usr/bin/env elle
(elle/epoch 9)
## Remove assertion import forms from all test/example files.
## Handles both single-line and multi-line (def {...} ((import-file "...assert...")))

(def glob-plugin (import "target/release/libelle_glob.so"))
(def do-glob (get glob-plugin :glob))

(def files (append
  (do-glob "tests/**/*.lisp")
  (do-glob "examples/**/*.lisp")))

## Find the end of a balanced form starting at the given paren.
## Returns the index after the closing delimiter.
(def find-close-paren (fn (s start)
  (letrec [walk (fn (i depth)
    (if (>= i (length s))
        i
        (let [ch (get s i)]
          (if (= ch "(")
              (walk (+ i 1) (+ depth 1))
              (if (= ch ")")
                  (if (= depth 1)
                      (+ i 1)
                      (walk (+ i 1) (- depth 1)))
                  (walk (+ i 1) depth))))))]
    (walk start 0))))

## Remove the assertion import form from file content.
## Looks for (def {... :assert-... } ((import-file "...assert...")))
(def remove-import (fn (content)
  (let [idx (string/find content "(def {:assert-")]
    (if (nil? idx)
        content
        (let [end (find-close-paren content idx)]
          ## Also remove trailing newline if present
          (let [end2 (if (and (< end (length content))
                              (= (get content end) "\n"))
                          (+ end 1)
                          end)]
            (string/join [(slice content 0 idx)
                          (slice content end2 (length content))] "")))))))

(def @cleaned 0)

(each f in files
  (when (and (not (string/ends-with? f "assert.lisp"))
             (not (string/ends-with? f "assertions.lisp"))
             (not (string/contains? f "AGENTS")))
    (let [content (slurp f)]
      (when (string/contains? content "(def {:assert-")
        (def new-content (remove-import content))
        (spit f new-content)
        (print (string/join ["cleaned: " f] ""))
        (assign cleaned (+ cleaned 1))))))

(print (string/join ["total: " (string cleaned) " files cleaned"] ""))
