(elle/epoch 18)
## audited: 2026-11-23
## A file can have its own process: what a child's exit status becomes in the store, and that the run goes on after one dies.
## tests/runner/overview.md
## docs/test-runner.md
##
## The counter-factual: worker threads share one process, so the first file
## that dies on a signal takes the runner down, and every result it had not
## written yet goes with it. Each scenario reads a row that could not exist
## under the shared-process runner.

(include-file "lib/harness.lisp")

# A file that kills its own process with SIGABRT.
#
# The trap: `os/sig-raise` resolves against the signals a program may send, and
# the fault set is not in it, so the fixture reaches one through `sh`, where
# `$PPID` is the child. SIGSEGV and SIGBUS are no use either: Rust's
# stack-overflow guard handles both, and a sent one carries no faulting
# instruction to re-raise on, so the process survives it.
(def aborts
  (string "(def k (subprocess/exec \"sh\" [\"-c\" \"kill -s ABRT $PPID\"]))\n"
          "(subprocess/wait k)\n(ev/sleep 10)\n"))

(with-temp-dir dir
  (defn isolate [name flags ms files]
    "`elle test --isolate FLAGS` over FILES, a list of [name source], into a
     store of its own under a budget of MS."
    (elle-test (path/join dir (string name ".db"))
               ["--isolate" flags "--timeout" (string ms)
                ;(map (fn [[file src]] (write-fixture dir file src)) files)]))

  (defn results [db]
    "Every result of the latest run, with the file it belongs to."
    (rows db
          (string "SELECT f.file AS file, r.tier AS tier, r.status AS status, "
                  "r.reason AS reason, r.signal AS sig FROM result r "
                  "JOIN form f ON f.hash = r.form_hash WHERE r.run_id = "
                  latest-run)))

  (defn of [res file]
    "The one row of RES whose file ends in FILE."
    (let [hits (filter (fn [row] (string/ends-with? row:file file)) res)]
      (assert (= (length hits) 1) (string "one row for " file ": " res))
      (first hits)))

  (defn kinds [db]
    "The asset kinds the latest run's results carry, with whether each holds bytes."
    (rows db
          (string "SELECT a.kind AS kind, (a.size > 0) AS sized FROM asset a "
                  "JOIN result r ON r.id = a.result_id WHERE r.run_id = "
                  latest-run)))

  (eprintln "scenario: an aborting file is recorded and the run completes")
  (let [r (isolate "abort" "" 30000
                   [["aborts.lisp" aborts]
                    ["passes.lisp" "(assert true \"a file that passes\")\n"]])
        run (get (rows r:db
                       (string "SELECT (finished_at IS NOT NULL) AS done, "
                               "tiers FROM run WHERE id = " latest-run)) 0)
        res (results r:db)
        died (of res "aborts.lisp")]
    (assert (not (= r:exit 0)) (string "a fail gates the run; stderr: " r:err))
    (assert (= run:done 1) "the run must complete after a child dies")
    (assert (= run:tiers "process")
            (string "an isolated run records the process tier: " run))
    (assert (= died:status "fail")
            (string "the aborting file is one fail: " died))
    (assert (string/contains? died:reason "SIGABRT")
            (string "the fail names the signal that killed the child: " died))
    (assert (= died:sig ":sigabrt")
            (string "and records it as the result's signal: " died))
    (assert (= ((of res "passes.lisp") :status) "pass")
            "the run must go on to the next path"))

  (eprintln "scenario: a nonzero exit is a fail naming the code")
  (let [r (isolate "exit" "" 30000 [["exits.lisp" "(os/exit 3)\n"]])
        row (of (results r:db) "exits.lisp")]
    (assert (not (= r:exit 0)) "a non-zero child gates the run")
    (assert (and (= row:status "fail") (string/contains? row:reason "exit 3"))
            (string "a child's exit code is the reason: " row)))

  (eprintln "scenario: a child's output becomes an asset")
  (let [r (isolate "talks" "" 30000
                   [["talks.lisp"
                     "(println \"on stdout\")\n(eprintln \"on stderr\")\n"]])
        assets (kinds r:db)]
    (assert (= r:exit 0)
            (string "a chatty child that exits 0 passes; stderr: " r:err))
    (each kind ["stdout" "stderr"]
      (assert (any? (fn [a] (= a:kind kind)) assets)
              (string "the child's " kind " becomes an asset: " assets)))
    (assert (all? (fn [a] (= a:sized 1)) assets)
            (string "an asset that captured nothing would not be one: " assets)))

  # The trap: the budget has to clear the child's own startup, not just beat
  # its sleep. A debug binary loads its stdlib in about 0.3 s idle and several
  # times that on a loaded box, so a 1.5 s budget killed the child before its
  # println. Ten seconds is thirty times the idle cost and a twelfth of the
  # sleep, so both ends have room.
  (eprintln "scenario: a child over its budget is a timeout")
  (let [r (isolate "slow" "" 10000
                   [["slow.lisp" "(println \"starting\")\n(ev/sleep 120)\n"]])]
    (assert (not (= r:exit 0)) "a timeout gates the run")
    (assert (= ((of (results r:db) "slow.lisp") :status) "timeout")
            "a child over its budget is a timeout, not a fail")
    # What it printed before the kill is the only account of where it was.
    (assert (any? (fn [a] (= a:kind "stdout")) (kinds r:db))
            "a killed child keeps what it printed"))

  # A gated child exits 0, so its exit status alone reads as a vacuous pass.
  # The binary prints `SKIP (gated):` on that path, and the runner reads it.
  (eprintln "scenario: a gated child is a skip carrying its reason")
  (let [r (isolate "gated" "" 30000
                   [["gated.lisp"
                     "(error (struct :error :gated :reason \"no widget here\"))\n"]])
        row (of (results r:db) "gated.lisp")]
    (assert (= r:exit 0) (string "a skip is not a failure; stderr: " r:err))
    (assert (= row:status "skip") (string "a gated child is a skip: " row))
    (assert (string/contains? row:reason "no widget here")
            (string "the skip carries the gate's reason: " row)))

  # Without this the mode the path exists for would be dropped, and every file
  # would run under the default configuration, green and meaningless.
  (eprintln "scenario: the flags reach the child")
  (let [r (isolate "flags" "--unicode=16.0" 30000
                   [["generation.lisp"
                     (string "(assert (= (vm/config :unicode) [16 0 0]) "
                             "\"the child runs under the flags it was given\")\n")]])]
    (assert (= r:exit 0)
            (string "the child must see --unicode=16.0; stderr: " r:err
                    " results: " (results r:db))))

  # The host records its argv and exits 0. The file exits 7 when anything runs
  # it, so a runner that ignored --host records a fail, and one that ran the
  # host with the wrong argv leaves a record that says so. The runner's own
  # verifier flag comes first, so the flags can override it
  # (docs/impl/verifier.md), and the stats flag follows it.
  (eprintln "scenario: the host runs each child with the flags and the path")
  (let [record (path/join dir "argv.txt")
        host (write-fixture dir "host.sh"
                            (string "#!/bin/sh\nprintf '%s\\n' \"$@\" > '"
                                    record "'\nexit 0\n"))
        file (write-fixture dir "exits7.lisp" "(os/exit 7)\n")]
    (subprocess/system "chmod" ["755" host])
    (let [r (elle-test (path/join dir "host.db")
                       ["--host" host "--isolate" "--unicode=16.0" file])
          row (of (results r:db) "exits7.lisp")]
      (assert (= r:exit 0)
              (string "the host exits 0, so the file passes; stderr: " r:err))
      (assert (= (slurp record)
                 (string "--jit-verify=on\n--dump=stats\n--unicode=16.0\n" file
                         "\n"))
              (string "the host gets the verifier and stats flags, the flags, "
                      "then the path: " (slurp record)))
      (assert (and (= row:tier "process") (= row:status "pass"))
              (string "a hosted child is a process row like any other: " row))))

  (eprintln "all isolation scenarios passed"))
