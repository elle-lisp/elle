(elle/epoch 18)
## audited: 2026-11-22
## An uncaught raise reports as an ordinary error whatever the raiser's inferred signal: exit 1, never an abort.
## tests/cli/overview.md
## docs/signals/inference.md
##
## The counter-factual: the raising callee inferred silent, and the call that
## completed it aborted the process as a "silence violation": a signal death
## with no exit code, a "panic:" line, and no runtime error report.

(include-file "../runner/lib/harness.lisp")

(defn reports-an-ordinary-error [source message]
  "Run SOURCE, which raises at top level with nothing to catch it, and require
   the report of an error whose message holds MESSAGE, and exit 1."
  (let [r (elle-run ["-e" source])]
    (assert (not (string/contains? r:err "panic"))
            (string "the run aborted for " source "; stderr: " r:err))
    (assert (= r:exit 1)
            (string "exit code " r:exit " for " source "; stderr: " r:err))
    (assert (and (string/contains? r:err "Runtime error")
                 (string/contains? r:err message))
            (string "no runtime error naming " message " for " source
                    "; stderr: " r:err))))

# `g` calls `f` in non-tail position, so `f`'s boundary is checked when the call
# completes inside `g`.
(eprintln "scenario: a raise from a function that once inferred silent is an ordinary error")
(reports-an-ordinary-error "(defn f [x] (def [a b] x) a) (defn g [] (def r (f 5)) r) (g)"
                           "destructuring: expected array")

(eprintln "scenario: a bound violation nothing catches is an ordinary error")
(reports-an-ordinary-error "(defn aps [f x] (bound f ||) (f x)) (defn g [] (def r (aps (get @[+] 0) 42)) r) (g)"
                           "bound: closure may emit {:error} but parameter is bounded to {}")

(eprintln "scenario: a raise under eval is an eval error")
(reports-an-ordinary-error "(eval '(begin (defn f [x] (def [a b] x) a) (defn g [] (def r (f 5)) r) (g)))"
                           "destructuring: expected array")

(eprintln "all uncaught-raise scenarios passed")
