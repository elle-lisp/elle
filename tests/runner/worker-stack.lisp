(elle/epoch 18)
## audited: 2026-11-22
## A worker that compiles a file has the main thread's stack, so a deep file compiles there too.
## tests/runner/overview.md
## docs/threads.md
##
## The runner ships a file's syntax to a worker, which compiles it with its own
## stdlib, and the front end's passes recurse depth-first over a long flat run
## of forms. Rust gives a spawned thread 2 MB of stack. The fixture overflows
## that, and the main thread's gating compile of the same file survives on its
## 8 MB, so only the worker's stack is under test. Nesting the forms instead
## would overflow the reader on the main thread first.
##
## The counter-factual: a worker on the default stack overflows mid-compile,
## and Rust aborts the runner. `RUST_MIN_STACK` is cleared, because a shell that
## exports it hides the default.
##
## The trap: how deep a file has to be depends on how far the build optimized
## its frames. A release build holds about 600 forms in 2 MB and 2000 in 8 MB, so
## 350 forms there overflow nothing, and a debug build overflows 8 MB before
## 1000. The build under test says which it is.

(include-file "lib/harness.lisp")

(def depth
  (if (= ((elle-run ["-e" "(print (elle/build-profile))"]) :out) "release")
    1000
    350))

(with-temp-dir dir
  (eprintln "scenario: a worker compiles a deep file without overflowing")
  (let [src (string (string/join (map (fn [i] (string "(def a" i " " i ")"))
                                      (range 0 depth)) "\n")
                    "\n(assert (= a0 0))\n")
        r (elle-test (path/join dir "s.db")
                     [(write-fixture dir "deep-worker-compile.lisp" src)]
                     :env (env-without "RUST_MIN_STACK"))]
    (assert (= r:exit 0)
            (string "a deep but valid file of " depth " forms must pass; exit "
                    r:exit " (-6 is a worker that overflowed its stack) — "
                    "stderr: " r:err)))

  (eprintln "all worker-stack scenarios passed"))
