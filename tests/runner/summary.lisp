(elle/epoch 18)
## audited: 2026-11-24
## A run that passed prints its run line and its tally, and the run line ends with the run's wall time and peak.
## tests/runner/overview.md
## docs/testing.md
##
## A suite pass prints one summary per batch, so every line a passing batch
## prints repeats a few dozen times in one CI step. The counter-factual: a
## passing run printed two heap-gauge blocks after its tally, and a reader
## scrolled past them to find the one batch that failed.
##
## The trap in the time and the peak: `wall_ms` and `max_rss_kb` are written
## after each file, so a run killed in its first file has neither, and a run
## line that printed `0.0 s` or `peak 0 MiB` for it would read as a run that
## measured itself. A run killed later has both, and its line prints them.

(include-file "lib/harness.lisp")

(defn seconds [ms]
  "A wall time as the run line spells it: seconds, a point, and tenths."
  (string (/ ms 1000) "." (mod (/ ms 100) 10) " s"))

(defn peak [kb]
  "A peak resident set as the run line spells it: whole MiB."
  (string "peak " (/ kb 1024) " MiB"))

(defn digits? [s]
  "Whether S is one or more decimal digits."
  (and (not (empty? s))
       (all? (fn [c] (string/contains? "0123456789" c)) (map string s))))

(defn a-time? [piece]
  "Whether PIECE is a time in the run line's spelling."
  (and (string/ends-with? piece " s")
       (let [parts (string/split (slice piece 0 (- (length piece) 2)) ".")]
         (and (= (length parts) 2) (digits? (get parts 0))
              (= (length (get parts 1)) 1) (digits? (get parts 1))))))

(defn a-peak? [piece]
  "Whether PIECE is a peak in the run line's spelling."
  (and (string/starts-with? piece "peak ") (string/ends-with? piece " MiB")
       (digits? (slice piece 5 (- (length piece) 4)))))

(defn pieces [line]
  "The pieces of LINE between its ` · ` separators."
  (string/split line " · "))

(defn ends-with-the-cost? [line]
  "Whether LINE ends with a time and then a peak."
  (let [ps (pieces line)
        n (length ps)]
    (and (>= n 2) (a-time? (get ps (- n 2))) (a-peak? (get ps (- n 1))))))

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
    (assert (ends-with-the-cost? (get said 0))
            (string "the run line ends with the run's wall time and peak: "
                    (get said 0)))
    # One result per tier the file ran on, so the count is the build's.
    (assert (and (string/contains? (get said 1) " pass · ")
                 (string/ends-with? (get said 1) " timeout"))
            (string "the second line is the tally: " (get said 1))))

  (eprintln "scenario: the run line prints the wall time and peak the run row records")
  (let [r (run-one "wall" "(assert true \"ok\")\n")
        ms (scalar r:db
                   (string "SELECT wall_ms AS c FROM run WHERE id = " latest-run))
        kb (scalar r:db
                   (string "SELECT max_rss_kb AS c FROM run WHERE id = "
                           latest-run))]
    (assert (= r:exit 0) "the run must gate green")
    (assert (and (not (nil? ms)) (> ms 0))
            (string "a run that compiled and ran a file took time, got " ms))
    # A booted runtime alone holds megabytes, so a peak under 1 MiB is a
    # reading that was never taken.
    (assert (and (not (nil? kb)) (>= kb 1024))
            (string "a run that booted a runtime held memory, got " kb " KiB"))
    (let [want (string " · " (seconds ms) " · " (peak kb))]
      (assert (string/ends-with? (get (lines r:err) 0) want)
              (string "the run line must print the recorded " ms " ms and " kb
                      " KiB as" want ": " (get (lines r:err) 0))))
    # --summary reads both back out of the store, with nothing re-run.
    (assert (= (summary r:db) (lines r:err))
            "--summary must re-print the summary the run printed"))

  # The tally counts one result per tier a file ran on, so it says nothing about
  # how many files a run took. The counter-factual: a 25-file batch on a
  # two-tier build printed `50 pass`, and the CI jobs read as dealing batches
  # of 10, 25, 50 and 100 files where there were two sizes.
  (eprintln "scenario: the run line counts the files the run selected")
  (let [db (path/join dir "files.db")
        pass "(assert true \"ok\")\n"
        r (elle-test db
                     [(write-fixture dir "a.lisp" pass)
                      (write-fixture dir "b.lisp" pass)])
        line (run-line (lines r:err))]
    (assert (= r:exit 0) (string "the run must gate green; stderr: " r:err))
    (assert (string/starts-with? line "elle test · run 1 of 1 · 2 files · ")
            (string "the run line counts the two files after the run: " line))
    (assert (= (run-line (summary db)) line)
            "--summary must re-print the count the run printed")
    (let [again (elle-test db [(write-fixture dir "c.lisp" pass)])
          line (run-line (lines again:err))]
      (assert (string/starts-with? line "elle test · run 2 of 2 · 1 file · ")
              (string "one file is `1 file`: " line))))

  (eprintln "scenario: a failing run lists its problems, its time and its peak")
  (let [r (run-one "fail" "(assert false \"the expected failure\")\n")
        said (lines r:err)]
    (assert (not (= r:exit 0)) (string "the run must gate red: " said))
    (assert (ends-with-the-cost? (run-line said))
            (string "a failing run's line ends with its wall time and peak too: "
                    (run-line said)))
    (assert (any? (fn [l] (string/contains? l "problem")) said)
            (string "a failing run says how many problems it found: " said))
    (assert (any? (fn [l]
                    (and (string/starts-with? (string/trim l) "fail")
                         (string/contains? l "one.lisp"))) said)
            (string "a failing run lists the file that failed: " said)))

  (eprintln "scenario: a run killed in its first file prints no time and no peak")
  (let [r (run-one "killed" "(os/sig-raise :sigkill)\n")]
    (assert (not (= r:exit 0)) "the fixture must kill the runner")
    (assert (= 1
               (scalar r:db
                       (string "SELECT (wall_ms IS NULL AND max_rss_kb IS NULL) "
                               "AS c FROM run WHERE id = " latest-run)))
            "a run killed before any file ended has no wall time and no peak")
    (let [line (run-line (summary r:db))]
      (assert (not (any? (fn [p] (or (a-time? p) (a-peak? p))) (pieces line)))
              (string "a killed run's line must not print a time or a peak it "
                      "never recorded: " line))))

  (eprintln "scenario: a run killed after a file prints the time and peak it recorded")
  (let [db (path/join dir "killed-later.db")
        r (elle-test db
                     [(write-fixture dir "first.lisp" "(assert true \"ok\")\n")
                      (write-fixture dir "kills.lisp"
                                     "(os/sig-raise :sigkill)\n")])
        ms (scalar db
                   (string "SELECT wall_ms AS c FROM run WHERE id = " latest-run))
        kb (scalar db
                   (string "SELECT max_rss_kb AS c FROM run WHERE id = "
                           latest-run))]
    (assert (= r:exit -9)
            (string "the fixture must SIGKILL the runner; exit " r:exit))
    (assert (and (not (nil? ms)) (not (nil? kb)))
            (string "the finished file left a wall time and a peak, got " ms
                    " ms and " kb " KiB"))
    (let [said (summary db)
          line (run-line said)]
      (assert (any? (fn [l] (string/contains? l "DID NOT COMPLETE")) said)
              (string "the summary must still call the run killed: " said))
      (assert (string/ends-with? line
                                 (string " · " (seconds ms) " · " (peak kb)))
              (string "the killed run's line must print the recorded " ms
                      " ms and " kb " KiB: " line))))

  (eprintln "all summary scenarios passed"))
