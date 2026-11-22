(elle/epoch 18)
## audited: 2026-11-22
## `--trace=boot` marks each boot phase and `--trace=compile` each pipeline phase, each with its duration.
## tests/cli/overview.md
## docs/config.md
##
## Each mark prints as `[trace:...] <label> <N>ms` on stderr. The trap:
## `--trace=` refuses an unknown keyword, so a binary without the `boot` keyword
## fails these runs outright rather than running without marks. The runs boot
## from source, because a start that hydrates a boot image skips the core,
## prelude and stdlib phases they name.

(include-file "../runner/lib/harness.lisp")

(with-temp-dir dir
  (def script (write-fixture dir "script.lisp" "(+ 1 2)\n"))

  (defn marks [keyword labels]
    "Run the script under --trace=KEYWORD, and require a mark with a duration
     for each of LABELS."
    (let [r (elle-run ["--boot-image=off" (string "--trace=" keyword) script])
          prefix (string "[trace:" keyword "]")]
      (assert (= r:exit 0) (string "elle --trace=" keyword " failed: " r:err))
      (each label labels
        (let [hits (filter (fn [l]
                             (and (string/starts-with? l prefix)
                                  (string/contains? l label))) (lines r:err))]
          (assert (not (empty? hits))
                  (string "no " prefix " mark for " label ": " r:err))
          (assert (string/ends-with? (string/trim (first hits)) "ms")
                  (string "the mark for " label " lacks a duration: "
                          (first hits)))))))

  (eprintln "scenario: boot marks attribute the startup phases")
  (marks "boot"
         ["primitives" "core" "prelude" "stdlib-compile" "stdlib-execute"])

  (eprintln "scenario: compile marks time the pipeline phases")
  (marks "compile" ["read" "expand" "analyze" "regions" "lower" "emit"]))

(eprintln "all trace-phases scenarios passed")
