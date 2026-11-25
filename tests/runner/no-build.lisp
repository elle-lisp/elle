(elle/epoch 18)
## audited: 2026-11-22
## A run under `elle test` has no build: it records no reading, judges none, gates on none, and refuses to re-pin.
## tests/runner/overview.md
## docs/ratchet.md
## docs/test-store.md
##
## The counter-factual: a row with no :build belonged to every build that had
## none of its own, so `elle test`, the runner a developer iterates with, judged
## a pool or MLIR reading against the reference build's footprint and failed
## it. `elle` names no build, so a run under it has no rows to judge against.
## The runner's cases that need a build are the rig's (rig/tests).

(include-file "lib/harness.lisp")

(with-temp-dir dir
  (def reads-42
    (string "(def r ((import \"std/ratchet\")))\n" "(def answer 42)\n"
            "(r:read \"answer\" :count answer)\n"))

  (defn bench [tag &opt src]
    "A working directory holding SRC as producer.lisp, by default a producer
     of several forms that reads 42, and its ledger, which pins 41: a
     regression on any build that judged it."
    (let [home (path/join dir tag)]
      (file/mkdir-all (path/join home "tests" "ledger"))
      (write-fixture home "producer.lisp" (or src reads-42))
      (write-fixture home "tests/ledger/producer.lisp"
                     (string "(elle/epoch 13)\n(producer \"producer.lisp\")\n"
                             "[\"answer\" :count 41]\n"))
      home))

  (defn run [home args]
    "`elle test ARGS producer.lisp` from HOME, whose tests/ledger a run with a
     build would judge against."
    (elle-test (path/join home "s.db") [;args "producer.lisp"] :cwd home))

  (defn measurements [home]
    (scalar (path/join home "s.db") "SELECT count(*) AS c FROM measurement"))

  (eprintln "scenario: a run with no build records no reading and gates on none")
  (let [home (bench "nobuild")
        r (run home [])]
    (assert (= r:exit 0)
            (string "a reading that would regress gates nothing with no build "
                    "to judge it; stderr: " r:err))
    (assert (= (measurements home) 0) "no measurement row is written")
    (assert (string/contains? r:err "no build")
            (string "the summary says the readings were neither recorded nor "
                    "judged, so a green run does not read as a passed gate: "
                    r:err)))

  (eprintln "scenario: an isolated child of elle test records none either")
  (let [home (bench "nobuild-child")
        r (run home ["--isolate" ""])]
    (assert (= r:exit 0) (string "the child passes; stderr: " r:err))
    (assert (= (measurements home) 0)
            "a child of a run with no build records nothing"))

  # The charge is the runner's own reading, never a line a file printed, and a
  # run with no build records it no more than it records a line. The file reads
  # nothing itself, so the summary's line can come from the charge alone.
  (eprintln "scenario: a charge with no build records none and says so")
  (let [home (bench "nobuild-charge" "(assert true \"reads nothing\")\n")
        r (run home ["--charge"])]
    (assert (= r:exit 0)
            (string "the charge gates nothing with no build to judge it; "
                    "stderr: " r:err))
    (assert (= (measurements home) 0) "no measurement row is written")
    (assert (string/contains? r:err "no build")
            (string "the summary says the readings were neither recorded nor "
                    "judged: " r:err)))

  (eprintln "scenario: --repin with no build refuses and leaves the ledger alone")
  (let [home (bench "nobuild-repin")
        ledger (path/join home "tests" "ledger" "producer.lisp")
        before (slurp ledger)
        r (run home ["--repin"])]
    (assert (not (= r:exit 0))
            (string "--repin with no build refuses; stderr: " r:err))
    (assert (and (string/contains? r:err "no build")
                 (string/contains? r:err "elle-rig test"))
            (string "and says why, and what to run instead: " r:err))
    (assert (= (slurp ledger) before) "the ledger is byte for byte what it was"))

  # The rig registers `elle/build` on the runtimes it builds; a user build has
  # no such primitive. Pinning the name's absence guards against the primitive
  # moving into the shipped runtime.
  (eprintln "scenario: elle names no build")
  (let [r (elle-run ["-e" "(elle/build)"])]
    (assert (and (not (= r:exit 0))
                 (string/contains? r:err "undefined variable")
                 (string/contains? r:err "elle/build"))
            (string "`elle/build` is an undefined variable under elle: " r:err)))

  (eprintln "all no-build scenarios passed"))
