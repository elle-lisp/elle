(elle/epoch 18)
## audited: 2026-11-22
## What make says a variable expands to and a target will run, read without running the target.
## tests/runner/overview.md
## docs/analysis/ci.md
##
## Splice this after lib/harness.lisp, whose env-without it reads.
##
## A recipe reaches its flags through variables: a value can sit inside an
## `ifdef`, come from `$(shell …)`, or name another variable. A test that
## parsed the Makefile would be a second `make` that can disagree with the
## first, so these ask `make`. Each call runs with no CI marker, no `JOBS`, and
## no parent make's flags, so the answer does not depend on who runs the test.

(def make-env
  (env-without "GITHUB_ACTIONS" "JOBS" "MAKEFLAGS" "MFLAGS" "MAKELEVEL"))

(defn make [args &named env]
  "Run `make ARGS` from the working directory, and answer what it left. ENV
   replaces the cleared slate, for a test about a variable in it."
  (subprocess/system "make" ["--no-print-directory" ;args]
                     {:env (or env make-env)}))

(defn make-var [name]
  "The value of the Makefile variable NAME, as make expands it."
  (let [r (make [(string "print-" name)])]
    (assert (= r:exit 0) (string "`make print-" name "` failed: " r:stderr))
    (string/trim r:stdout)))

(defn make-dry-run [target]
  "The commands `make TARGET` would run, each variable expanded."
  (let [r (make ["--dry-run" target])]
    (assert (= r:exit 0)
            (string "`make --dry-run " target "` failed: " r:stderr))
    r:stdout))
