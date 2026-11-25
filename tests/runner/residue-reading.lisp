(elle/epoch 18)
## audited: 2026-11-24
## Under `elle-rig test --isolate`, a child that passed gives one residue reading, and each pass owes a file its own axis alone.
## tests/runner/overview.md
## docs/test-cost.md
## docs/ratchet.md
##
## The counter-factuals: a runner that records the residue column and judges
## nothing, so a child whose teardown leaves a region more passes; a failed
## child that owes its row, so one failure reads twice; and a pass that owes
## every row about its file, so the isolated pass reads the charge's `objects`
## row as missing, and the charge pass the `residue` row.
##
## Each scenario runs from a working directory of its own, whose tests/ledger
## is the only ledger the run reads. The pin is the residue this build's child
## reads, taken from a run with no ledger, so the file holds on every build.
##
## The trap: a row with no `:build` belongs to the reference build alone, so on
## any other build the run judges nothing, and a pinned reading's verdict is
## `none`. Each row names the build the run with no ledger recorded, which on
## the reference build means the same as no `:build`.

(include-file "lib/harness.lisp")

(with-temp-dir dir
  (defn home [tag rows]
    "A working directory holding pass.lisp and fail.lisp, and, when ROWS is
     not nil, a ledger for the producer `elle test` holding ROWS."
    (let [h (path/join dir tag)]
      (file/mkdir-all h)
      (write-fixture h "pass.lisp" "(assert true \"passes\")\n")
      (write-fixture h "fail.lisp" "(assert false \"fails\")\n")
      (when rows
        (file/mkdir-all (path/join h "tests" "ledger"))
        (write-fixture h "tests/ledger/runner.lisp"
                       (string "(elle/epoch 18)\n(producer \"elle test\")\n"
                               (string/join rows "\n") "\n")))
      h))

  (defn run [h args]
    "`elle-rig test ARGS` from H, into a store of its own."
    (rig-test (path/join h "s.db") args :cwd h))

  (defn readings [h]
    "Every measurement row of H's latest run, as subject, axis, value, unit
     and verdict, the verdict \"none\" when unjudged."
    (rows (path/join h "s.db")
          (string "SELECT subject, axis, value, unit, "
                  "coalesce(verdict, 'none') AS verdict FROM measurement "
                  "WHERE run_id = " latest-run " ORDER BY axis")))

  (defn one-reading [h what]
    "H's latest run's single measurement row; WHAT names the scenario."
    (let [rs (readings h)]
      (assert (= (length rs) 1)
              (string what ": one reading, got " (length rs) ": " rs))
      (first rs)))

  (eprintln "scenario: with no ledger, a passing child's residue lands unjudged")
  (def [residue build]
    (let [h (home "unjudged" nil)
          r (run h ["--isolate" "" "pass.lisp"])
          m (one-reading h "a passing child")
          column (scalar (path/join h "s.db")
                         (string "SELECT residue_regions AS c FROM result "
                                 "WHERE run_id = " latest-run))]
      (assert (= r:exit 0) (string "the child passes; stderr: " r:err))
      (assert (= m:subject "pass.lisp") (string "the subject is the path: " m))
      (assert (= m:axis "residue") (string "the axis is residue: " m))
      (assert (= m:unit "regions") (string "the unit is regions: " m))
      (assert (= m:verdict "none") (string "no ledger names the producer: " m))
      (assert (integer? column) "the child's result records its residue")
      (assert (= (integer m:value) column)
              (string "the reading is the result's residue_regions, " column
                      ": " m))
      [column
       (scalar (path/join h "s.db")
               (string "SELECT build AS c FROM run WHERE id = " latest-run))]))

  (defn row [subject axis bound]
    "A ledger row for SUBJECT on AXIS, pinned at BOUND, of this build."
    (string "[\"" subject "\" :" axis " " bound " :build \"" build "\"]"))

  (eprintln "scenario: a residue at its pin is ok")
  (let [h (home "pinned" [(row "pass.lisp" "residue" residue)])
        r (run h ["--isolate" "" "pass.lisp"])]
    (assert (= r:exit 0) (string "a reading at its pin gates green: " r:err))
    (assert (= (get (one-reading h "a pinned child") :verdict) "ok")
            "the reading meets its row"))

  (eprintln "scenario: a residue off its pin gates")
  (let [h (home "off" [(row "pass.lisp" "residue" (+ residue 1))])
        r (run h ["--isolate" "" "pass.lisp"])]
    (assert (not (= r:exit 0))
            (string "a reading below its pin is stale, and gates: " r:err))
    (assert (= (get (one-reading h "an off-pin child") :verdict) "stale")
            "the pin has slipped behind the tree"))

  (eprintln "scenario: a failed child gives no reading and owes no row")
  (let [h (home "failed" [(row "fail.lisp" "residue" 0)])
        r (run h ["--isolate" "" "fail.lisp"])]
    (assert (not (= r:exit 0)) "the failed child gates")
    (assert (empty? (readings h))
            (string "a failed child reads nothing and owes nothing: "
                    (readings h))))

  (eprintln "scenario: the isolated pass does not owe a file its charge")
  (let [h (home "no-charge"
                [(row "pass.lisp" "objects" 0)
                 (row "pass.lisp" "residue" residue)])
        r (run h ["--isolate" "" "pass.lisp"])]
    (assert (= r:exit 0)
            (string "the objects row is the charge pass's, not missing here: "
                    r:err))
    (assert (= (get (one-reading h "an isolated pass") :axis) "residue")
            "the isolated pass reads the residue alone"))

  (eprintln "scenario: the charge pass does not owe a file its residue")
  (let [objects (let [h (home "charge-unjudged" nil)
                      r (run h ["--charge" "pass.lisp"])]
                  (assert (= r:exit 0) (string "the charge runs: " r:err))
                  (integer (get (one-reading h "a charge") :value)))
        h (home "no-residue"
                [(row "pass.lisp" "objects" objects)
                 (row "pass.lisp" "residue" residue)])
        r (run h ["--charge" "pass.lisp"])]
    (assert (= r:exit 0)
            (string "the residue row is the isolated pass's, not missing here: "
                    r:err))
    (assert (= (get (one-reading h "a charge pass") :axis) "objects")
            "the charge pass reads the objects alone"))

  (eprintln "all residue-reading scenarios passed"))
