(elle/epoch 18)
## audited: 2026-11-24
## An isolated child's reclamation report lands as one child gauge row per counter, the last line of each name taken.
## tests/runner/overview.md
## docs/test-gauges.md
##
## The counter-factual: a runner that writes no child row passes every other
## runner test, because an isolated file's verdict is its exit status. One that
## writes the runner's own counters, or zeros, under the child heap still
## writes 19 rows, so the adopting child must outcount the plain one. One that
## takes the first line of a name records whatever the program printed first.

(include-file "lib/harness.lisp")

# Every counter the report prints: the gauges less objects, regions and pages.
(def names
  ["region-frees" "page-frees" "object-frees" "one-page-frees" "empty-frees"
   "one-object-frees" "few-object-frees" "many-object-frees" "adopts"
   "adopts-into-empty" "owned" "owned-frees" "owned-free-pages"
   "owned-free-objects" "owned-one-page-frees" "rescues" "rescue-survivors"
   "extracts" "reparents"])

# A function whose push adopts the pushed array into the container. The trap:
# the array holds the call's argument. An array of constants is built once
# when the unit starts, so the calls would push one array and never adopt it.
(def adopting
  (string "(defn keep-one [x]\n  (let [c (@array)]\n"
          "    (push c (array x 2))\n    nil))\n" "(var i 0)\n"
          "(while (< i 200)\n  (keep-one i)\n  (assign i (+ i 1)))\n"
          "(assert true \"ok\")\n"))

(def plain "(assert true \"ok\")\n")

(defn child-rows [db]
  "The latest run's child-heap gauge rows, as kind, delta, reading and file."
  (rows db
        (string "SELECT kind, delta, reading, file FROM gauge WHERE heap = 'child' "
                "AND run_id = " latest-run " ORDER BY kind")))

(defn child-delta [db kind]
  "The latest run's child row of KIND, as its delta."
  (let [hit (filter (fn [row] (= row:kind kind)) (child-rows db))]
    (assert (= (length hit) 1)
            (string "one child row of " kind ", got " (child-rows db)))
    (get (first hit) :delta)))

(defn kinds [db]
  (string/join (map (fn [row] row:kind) (child-rows db)) ","))

(with-temp-dir dir
  (defn isolate [name src extra]
    "`elle test --isolate ''` over one fixture holding SRC, with EXTRA flags,
     into a store of its own."
    (elle-test (path/join dir (string name ".db"))
               ["--isolate" "" ;extra
                (write-fixture dir (string name ".lisp") src)]))

  (defn hosted [name script]
    "`elle test --host HOST --isolate ''` over one passing fixture, where HOST
     is a program of sh that runs SCRIPT."
    (let [host (write-fixture dir (string name ".sh")
                              (string "#!/bin/sh\n" script "\n"))]
      (subprocess/system "chmod" ["755" host])
      (elle-test (path/join dir (string name ".db"))
                 ["--host" host "--isolate" ""
                  (write-fixture dir (string name ".lisp") plain)])))

  (eprintln "scenario: a child records one child row per counter, with no reading")
  (let [r (isolate "plain" plain [])
        got (child-rows r:db)]
    (assert (= r:exit 0) (string "the child must pass; stderr: " r:err))
    (assert (= (kinds r:db) (string/join (sort names) ","))
            (string "one child row per counter the report prints, got " got))
    (assert (all? (fn [row] (nil? row:reading)) got)
            (string "a child row carries no reading: " got))
    (assert (all? (fn [row] (string/ends-with? row:file "plain.lisp")) got)
            (string "every child row names the child's file: " got))
    (assert (> (child-delta r:db "region-frees") 0)
            "the child freed regions, so its counts are not vacuous"))

  (eprintln "scenario: the rows carry the child's own counts")
  (let [quiet (child-delta ((isolate "quiet" plain []) :db) "adopts")
        r (isolate "adopting" adopting [])]
    (assert (= r:exit 0) (string "the adopting child must pass; stderr: " r:err))
    (let [busy (child-delta r:db "adopts")]
      (assert (>= (- busy quiet) 200)
              (string "the child's 200 adoptions must reach its row: plain "
                      quiet ", adopting " busy))))

  # The program prints a line that looks like the report's. The report prints
  # at exit, so its own line comes last.
  (eprintln "scenario: the last line of a name is the child's own")
  (let [r (isolate "mimic"
                   (string "(eprintln \"[stats] reclaim adopts=999999\")\n"
                           plain) [])]
    (assert (= r:exit 0) (string "the child must pass; stderr: " r:err))
    (assert (not (= (child-delta r:db "adopts") 999999))
            (string "the row records the report's count, not the program's: "
                    (child-delta r:db "adopts"))))

  (eprintln "scenario: a row is read from the last line of its name, and a name that is no gauge has none")
  (let [r (hosted "seven"
                  (string "echo '[stats] reclaim adopts=999' >&2\n"
                          "echo '[stats] reclaim bogus=3' >&2\n"
                          "echo '[stats] reclaim adopts=7' >&2\nexit 0"))]
    (assert (= r:exit 0) (string "the host exits 0; stderr: " r:err))
    (assert (= (kinds r:db) "adopts")
            (string "the host printed one gauge, got " (child-rows r:db)))
    (assert (= (child-delta r:db "adopts") 7)
            (string "the row records the last line's count: "
                    (child-delta r:db "adopts")))
    (eprintln "scenario: an import keeps the child rows")
    (let [here (path/join dir "here.db")
          i (elle-test here ["--import" r:db])]
      (assert (= i:exit 0) (string "the import failed; stderr: " i:err))
      (assert (= (child-delta here "adopts") 7)
              (string "the imported store carries the child row: "
                      (child-rows here)))))

  (eprintln "scenario: a child that prints no report records no child row")
  (let [r (hosted "silent" "exit 0")]
    (assert (= r:exit 0) (string "the host exits 0; stderr: " r:err))
    (assert (empty? (child-rows r:db))
            (string "a silent child records no child row: " (child-rows r:db))))

  # The report prints from an exit handler, which os/exit runs.
  (eprintln "scenario: a child that ends in os/exit records its rows")
  (let [r (isolate "exits" "(os/exit 0)\n" [])]
    (assert (= r:exit 0) (string "the child exits 0; stderr: " r:err))
    (assert (= (kinds r:db) (string/join (sort names) ","))
            (string "a child that ends in os/exit still reports: "
                    (child-rows r:db))))

  (eprintln "scenario: a killed child records no child row")
  (let [r (isolate "slow" "(ev/sleep 5)\n" ["--timeout" "500"])]
    (assert (not (= r:exit 0)) "a timeout gates non-zero")
    (assert (empty? (child-rows r:db))
            (string "a killed child records no child row: " (child-rows r:db))))

  (eprintln "scenario: an in-process run records no child row")
  (let [db (path/join dir "worker.db")
        r (elle-test db [(write-fixture dir "worker.lisp" plain)])]
    (assert (= r:exit 0) (string "the form must pass; stderr: " r:err))
    (assert (empty? (child-rows db))
            (string "no worker run records a child row: " (child-rows db))))

  (eprintln "all child-gauges scenarios passed"))
