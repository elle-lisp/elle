(elle/epoch 18)
## audited: 2026-11-22
## A run that passed prints its run line and its tally, and the run line ends with the run's wall time.
## tests/runner/overview.md
## docs/testing.md
##
## A suite pass prints one summary per batch, so every line a passing batch
## prints repeats a few dozen times in one CI step. The counter-factual: a
## passing run printed two heap-gauge blocks after its tally, and a reader
## scrolled past them to find the one batch that failed.
##
## The trap in the time: `wall_ms` is written at completion, so a killed run has
## none, and a run line that printed `0.0 s` for it would read as a run that
## finished.

(include-file "lib/harness.lisp")

(defn seconds [ms]
  "A wall time as the run line spells it: seconds, a point, and tenths."
  (string (/ ms 1000) "." (mod (/ ms 100) 10) " s"))

(defn ends-with-a-time? [line]
  "Whether LINE ends with a time in the run line's spelling."
  (and (string/ends-with? line " s")
       (let [word (last (string/split (slice line 0 (- (length line) 2)) " "))
             parts (string/split word ".")
             digits? (fn [s]
                       (and (not (empty? s))
                            (all? (fn [c] (string/contains? "0123456789" c))
                                  (map string s))))]
         (and (= (length parts) 2) (digits? (get parts 0))
              (= (length (get parts 1)) 1) (digits? (get parts 1))))))

(defn run-line [said]
  "The run line among SAID."
  (let [hits (filter (fn [l] (string/starts-with? l "elle test · run ")) said)]
    (assert (not (empty? hits)) (string "no run line in " said))
    (first hits)))

(with-temp-dir dir
  (defn run-one [name src]
    "`elle test` over one file holding SRC, into a fresh store."
    (elle-test (path/join dir (string name ".db"))
               [(write-fixture dir "one.lisp" src)]))

  (defn summary [db]
    (lines ((elle-test db ["--summary"]) :err)))

  (eprintln "scenario: a passing run prints its run line and its tally, and nothing else")
  (let [r (run-one "pass" "(assert true \"ok\")\n")
        said (lines r:err)]
    (assert (= r:exit 0) (string "the run must gate green; stderr: " r:err))
    (assert (= (length said) 2)
            (string "a passing run prints its run line and its tally: " said))
    (assert (string/starts-with? (get said 0) "elle test · run 1 of 1")
            (string "the first line is the run line: " (get said 0)))
    (assert (ends-with-a-time? (get said 0))
            (string "the run line ends with the run's wall time: " (get said 0)))
    # One result per tier the file ran on, so the count is the build's.
    (assert (and (string/contains? (get said 1) " pass · ")
                 (string/ends-with? (get said 1) " timeout"))
            (string "the second line is the tally: " (get said 1))))

  (eprintln "scenario: the run line prints the wall time the run row records")
  (let [r (run-one "wall" "(assert true \"ok\")\n")
        ms (scalar r:db
                   (string "SELECT wall_ms AS c FROM run WHERE id = " latest-run))]
    (assert (= r:exit 0) "the run must gate green")
    (assert (and (not (nil? ms)) (> ms 0))
            (string "a run that compiled and ran a file took time, got " ms))
    (assert (string/ends-with? (get (lines r:err) 0)
                               (string " · " (seconds ms)))
            (string "the run line must print the recorded " ms " ms as "
                    (seconds ms) ": " (get (lines r:err) 0)))
    # --summary reads the time back out of the store, with nothing re-run.
    (assert (= (summary r:db) (lines r:err))
            "--summary must re-print the summary the run printed"))

  (eprintln "scenario: a failing run lists its problems and its time")
  (let [r (run-one "fail" "(assert false \"the expected failure\")\n")
        said (lines r:err)]
    (assert (not (= r:exit 0)) (string "the run must gate red: " said))
    (assert (ends-with-a-time? (run-line said))
            (string "a failing run's line ends with its wall time too: "
                    (run-line said)))
    (assert (any? (fn [l] (string/contains? l "problem")) said)
            (string "a failing run says how many problems it found: " said))
    (assert (any? (fn [l]
                    (and (string/starts-with? (string/trim l) "fail")
                         (string/contains? l "one.lisp"))) said)
            (string "a failing run lists the file that failed: " said)))

  (eprintln "scenario: a killed run prints no time")
  (let [r (run-one "killed" "(os/sig-raise :sigkill)\n")]
    (assert (not (= r:exit 0)) "the fixture must kill the runner")
    (assert (= 1
               (scalar r:db
                       (string "SELECT (wall_ms IS NULL) AS c FROM run "
                               "WHERE id = " latest-run)))
            "a run that never completed has no wall time")
    (let [line (run-line (summary r:db))]
      (assert (not (string/ends-with? line " s"))
              (string "a killed run's line must not print a time it never "
                      "recorded: " line))))

  (eprintln "all summary scenarios passed"))
