(elle/epoch 18)
## audited: 2026-11-22
## What a load of a file reports, and the location of a program that has no file.
## tests/cli/overview.md
## docs/modules.md
## docs/config.md

(include-file "../runner/lib/harness.lisp")
(include-file "lib/modules.lisp")

(with-temp-dir dir
  (write-module dir "m.lisp" "42")

  # `import/load-file` takes a path as `file/read` does, so a relative path
  # resolves against the working directory.
  (eprintln "scenario: a load on stdin resolves against the working directory")
  (let [r (run-in dir ["-"] "(print (import/load-file \"m.lisp\"))")]
    (assert (= r:exit 0) (string "the load succeeds; stderr: " r:err))
    (assert (= (string/trim r:out) "42") (string "printed " r:out)))

  (eprintln "scenario: a program with no file has a nil location file")
  (let [stdin (run-in dir ["-"] "(print (get (meta/location) :file))")
        dash-e (run-in dir ["-e" "(print (get (meta/location) :file))"] "")]
    (assert (= stdin:exit 0) (string "stdin runs; stderr: " stdin:err))
    (assert (= (string/trim stdin:out) "nil") "stdin has no file")
    (assert (= dash-e:exit 0) (string "-e runs; stderr: " dash-e:err))
    (assert (= (string/trim dash-e:out) "nil") "-e has no file"))

  # A failed load keeps nothing. The counter-factual kept the failure beside
  # the code, and the second load answered without reading the file.
  (eprintln "scenario: a failed load fails again on the next load")
  (write-module dir "bad.lisp" "(no-such-name)")
  (let [r (run-in dir ["-"]
                  (string "(def [a? _] (protect (import/load-file \"bad.lisp\")))\n"
                          "(def [b? _] (protect (import/load-file \"bad.lisp\")))\n"
                          "(print [a? b?])"))]
    (assert (= r:exit 0) (string "the program runs; stderr: " r:err))
    (assert (= (string/trim r:out) "[false false]") (string "printed " r:out)))

  # `compile/analyze` runs no code, and reads no file a literal load names, so
  # two programs whose literal loads name each other analyze. The
  # counter-factual compiled each literal load during analysis, and recursed
  # until the process aborted on a stack overflow.
  (eprintln "scenario: analyzing a cycle of literal loads ends")
  (let [a (path/join dir "a.lisp")
        b (path/join dir "b.lisp")]
    (file/write a
                (string "(def b ((import/load-file \"" b
                        "\")))\n(fn [] {:b b})\n"))
    (file/write b
                (string "(def a ((import/load-file \"" a
                        "\")))\n(fn [] {:a a})\n"))
    (let [r (run-in dir ["-"]
                    (string "(compile/analyze (file/read \"" a "\") {:file \"" a
                            "\"})\n(print \"analyzed\")\n"))]
      (assert (= r:exit 0) (string "the analysis returns; stderr: " r:err))
      (assert (= (string/trim r:out) "analyzed") (string "printed " r:out)))))

(eprintln "all module-load scenarios passed")
