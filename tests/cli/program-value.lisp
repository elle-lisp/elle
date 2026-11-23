(elle/epoch 18)
## audited: 2026-11-22
## A start that answers with a heap value gives its owning reference back, so the run leaves no region behind.
## tests/cli/overview.md
## docs/impl/region/rules.md
##
## A script run answers once, and a REPL session answers form by form, so a
## kept reference reads there as a slope rather than a fixed cost. Every start
## passes `--dump=stats`, whose teardown line counts the live regions.

(include-file "../runner/lib/harness.lisp")

(def teardown-line "[stats] live regions after teardown: ")

(defn residue [r what]
  "The live regions run R reported after teardown. R must have exited 0."
  (assert (= r:exit 0) (string what ": the start failed: " r:err))
  (let [line (find (fn [l] (string/starts-with? l teardown-line)) (lines r:err))]
    (assert line (string what ": no residue line; stderr: " r:err))
    (parse-int (first (string/split (slice line (length teardown-line)) " ")))))

(defn repl-residue [session]
  "The residue of a REPL session that runs SESSION."
  (residue (elle-feed ["--dump=stats"] session) session))

# One program value per heap shape a top-level form answers with. A run whose
# last form is one of these has no binding to root the value, so the binary's
# own release is the only thing that balances the return convention's mint.
(def answers ["(list 1 2 3)" "\"str\"" "[1 2 3]" "{:a 1}" "(fn [x] x)"])

# The counter-factual is `(begin ANSWER nil)`, measured beside each answer. It
# allocates the same value and drops it inside the program, where the
# compiler's own release reclaims it. So a difference between the two readings
# is the hand-off alone: `(range 300)` reads what `(range 3)` reads.
(eprintln "scenario: a run answering with a heap value leaves no residue")
(each answer answers
  (let [discarded (string "(begin " answer " nil)")]
    (assert (= 0
               (residue (elle-run ["--dump=stats" (string "--eval:" discarded)])
                        discarded))
            (string discarded ": a run that drops its value left a region"))
    (assert (= 0
               (residue (elle-run ["--dump=stats" (string "--eval:" answer)])
                        answer))
            (string answer ": the run kept the reference the return "
                    "convention handed it"))))

# One `run_source` serves all three entries, so a release that reaches one of
# them alone sits in the wrong place.
(eprintln "scenario: a file or stdin run answering with a heap value leaves no residue")
(with-temp-dir dir
  (each answer answers
    (let [script (write-fixture dir "answer.lisp" (string answer "\n"))]
      (assert (= 0 (residue (elle-run ["--dump=stats" script]) answer))
              (string answer ": run as a file"))
      (assert (= 0 (residue (elle-feed ["--dump=stats" "-"] answer) answer))
              (string answer ": run from stdin")))))

# A prompt form that binds nothing hands its value to the REPL alone. The
# counter-factual is the length: a host that keeps each run's reference leaks
# one region per form, so 3 forms and 6 forms read apart.
(eprintln "scenario: a REPL form answering with a heap value leaves no residue")
(each answer answers
  (let [short (repl-residue (string/repeat (string answer "\n") 3))
        long (repl-residue (string/repeat (string answer "\n") 6))]
    (assert (= short long)
            (string answer ": 3 forms left " short " regions and 6 left " long))
    (assert (= short 0) (string answer ": the session left " short " regions"))))

(eprintln "scenario: a REPL def leaves no residue")
(each answer answers
  (let [session (string "(def bound " answer ")\nbound\n")]
    (assert (= 0 (repl-residue session))
            (string session ": the session left a region"))))

# A destructuring def compiles to the def and a tuple of its leaf names, so the
# REPL releases the tuple and keeps each leaf.
(eprintln "scenario: a REPL destructuring def leaves no residue")
(each session ["(def [a b] [\"pp\" \"qq\"])\n"
               "(def {:x a :y b} {:x \"rr\" :y \"ss\"})\n"]
  (assert (= 0 (repl-residue session))
          (string session ": the session left a region")))

# The counter-factual is the scenario above, which cannot see this: a leaf
# registered against the tuple's reference reads as clean at teardown, and is
# freed with the tuple while the session still names it.
(eprintln "scenario: a destructured REPL binding outlives the tuple it came from")
(let [r (elle-feed ["--dump=stats"]
                   (string "(def [a b] [\"pp\" \"qq\"])\n"
                           "(def {:x c :y d} {:x \"rr\" :y \"ss\"})\n"
                           "(string/upcase a)\n(string/upcase b)\n"
                           "(string/upcase c)\n(string/upcase d)\n"))]
  (assert (= r:exit 0) (string "the session failed: " r:err))
  (each want ["\"PP\"" "\"QQ\"" "\"RR\"" "\"SS\""]
    (assert (string/contains? r:out want)
            (string "the session never printed " want ": " r:out))))

# The retry runs the form alone and prints nothing, so its release cannot ride
# on the print.
(eprintln "scenario: a REPL resolving a deferred form leaves no residue")
(assert (= 0
           (repl-residue (string "(defn ahead [] (behind))\n"
                                 "(defn behind [] \"s\")\n(ahead)\n")))
        "the retry left a region")

(eprintln "all program-value scenarios passed")
