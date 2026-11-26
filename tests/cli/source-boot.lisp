(elle/epoch 18)
## audited: 2026-11-25
## A source boot compiles the stdlib at every start, workers included, and writes no cache.
## tests/cli/overview.md
## docs/impl/image/boot.md
##
## `--boot-image=off` reads and stores no image, and nothing else beneath
## `--cache=` holds a compiled stdlib. So every start compiles as the first did.
## The counter-factual is a start that reuses what an earlier start wrote: it
## answers every call, and only its boot marks and its cache directory show it.

(include-file "../runner/lib/harness.lisp")

(defn marks [r what]
  "How many `[trace:boot]` lines of run R name WHAT."
  (length (filter (fn [l]
                    (and (string/starts-with? l "[trace:boot]")
                         (string/contains? l what))) (lines r:err))))

(with-temp-dir dir
  (def cache (path/join dir "cache"))
  (def script (write-fixture dir "script.lisp" "(+ 1 2)\n"))
  (def flags [(string "--cache=" cache) "--boot-image=off" "--trace=boot"])

  (eprintln "scenario: every source boot compiles the stdlib")
  (let [first (elle-run [;flags script])
        second (elle-run [;flags script])]
    (assert (= first:exit 0) (string "the first run failed: " first:err))
    (assert (= second:exit 0) (string "the second run failed: " second:err))
    (each r in [first second]
      (assert (= (marks r "stdlib-compile") 1)
              (string "a source boot marks one stdlib compile: " r:err)))
    (assert (= (marks second "cache hit") 0)
            (string "the second run loaded what the first stored: " second:err)))

  # The worker holds no image to inherit, so it compiles the stdlib too.
  (eprintln "scenario: a source boot's heavy worker compiles the stdlib and answers")
  (let [r (elle-run [;flags "-e"
                     "(println (sys/join (sys/spawn (fn [] (eval '(map inc [1 2 3]))))))"])]
    (assert (= r:exit 0) (string "the start failed: " r:err))
    (assert (= (string/trim r:out) "[2 3 4]")
            (string "the worker's stdlib answered " r:out))
    (assert (= (marks r "stdlib-compile") 2)
            (string "the start and its worker each compile the stdlib: " r:err))
    (assert (= (marks r "image-hydrate") 0)
            (string "a source boot's worker hydrated an image: " r:err)))

  (eprintln "scenario: a source boot writes nothing beneath its cache directory")
  (assert (or (not (file/exists? cache)) (empty? (file/ls cache)))
          (string "a source boot wrote " (file/ls cache) " beneath " cache))

  # A diagnostic that prints when nobody asked gets filtered out and ignored.
  (eprintln "scenario: phase marks stay quiet without the keyword")
  (let [r (elle-run [script])]
    (assert (= r:exit 0) (string "elle exited non-zero: " r:err))
    (each subsystem ["[trace:boot]" "[trace:compile]"]
      (assert (not (string/contains? r:err subsystem))
              (string "a " subsystem " mark printed without --trace: " r:err)))))

(eprintln "all source-boot scenarios passed")
