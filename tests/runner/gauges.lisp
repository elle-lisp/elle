(elle/epoch 18)
## audited: 2026-11-24
## The runner reads every heap gauge on its own heap and on its test code's heaps, per file.
## tests/runner/overview.md
## docs/test-gauges.md
##
## The counter-factual: a runner that samples nothing still passes every corpus
## test, because a leak in the harness is invisible to the harness. The test
## heap has the same blind spot one level down: what the test code allocates
## never reaches the runner's heap, so only a reading inside the worker sees it.
##
## The trap in the chain: two samples per file, one before and one after, would
## leave the rows written between them charged to nobody, and the gap is where
## the runner's own work lives. One reading per boundary is what makes the
## readings chain. Test-heap rows have no chain, so the chain leaves them out.

(include-file "lib/harness.lisp")

# Every gauge, in the order the runner reads them.
(def gauges
  ["objects" "regions" "pages" "region-frees" "page-frees" "object-frees"
   "one-page-frees" "empty-frees" "one-object-frees" "few-object-frees"
   "many-object-frees" "adopts" "adopts-into-empty" "owned" "owned-frees"
   "owned-free-pages" "owned-free-objects" "owned-one-page-frees" "rescues"
   "rescue-survivors" "extracts" "reparents"])

(defn this-run [db rest]
  "The rows REST selects from the latest run's gauges."
  (rows db (string "SELECT " rest " WHERE run_id = " latest-run)))

(defn per-kind [db heap]
  "The latest run's row and file counts per gauge on HEAP."
  (rows db
        (string "SELECT kind, count(*) AS n, count(DISTINCT file) AS files "
                "FROM gauge WHERE run_id = " latest-run " AND heap = '" heap
                "' GROUP BY kind")))

(defn one-row-per-gauge [db heap files what]
  "Require one row per gauge and per file on HEAP, FILES files in all."
  (let [got (per-kind db heap)]
    (assert (= (string/join (sort (map (fn [row] row:kind) got)) ",")
               (string/join (sort gauges) ","))
            (string what ": every gauge on the " heap " heap, got " got))
    (assert (all? (fn [row] (and (= row:n files) (= row:files files))) got)
            (string what ": one " heap "-heap row per gauge and file, got " got))))

(defn heap-rows [db]
  "How many of the latest run's gauge rows each heap holds."
  (get (this-run db
                 (string "sum(CASE WHEN heap = 'runner' THEN 1 ELSE 0 END) AS runner, "
                         "sum(CASE WHEN heap = 'test' THEN 1 ELSE 0 END) AS test "
                         "FROM gauge")) 0))

(with-temp-dir dir
  (defn run-files [name files extra green]
    "Run FILES, a list of [name source], in one `elle test` with EXTRA flags,
     and require the gate GREEN or red."
    (let [r (elle-test (path/join dir (string name ".db"))
                       [;(map (fn [[file src]] (write-fixture dir file src))
                              files) ;extra])]
      (assert (= (= r:exit 0) green)
              (string name ": the run must gate " (if green "green" "red")
                      "; stderr: " r:err))
      r))

  (def corpus
    (run-files "corpus"
               (map (fn [n] [n "(assert true \"ok\")\n"])
                    ["a-first.lisp" "b-second.lisp" "c-third.lisp"]) [] true))

  (eprintln "scenario: every file a run processed carries one row per gauge on each heap")
  (each heap ["runner" "test"]
    (one-row-per-gauge corpus:db heap 3 "the corpus"))

  # Every gauge, one boundary per file: two consecutive pairs each, asserted
  # so the chain cannot pass by finding no pairs to check.
  (eprintln "scenario: the runner heap's boundary readings chain with no gap")
  (let [row (get (rows corpus:db
                       (string "SELECT count(*) AS pairs, sum(CASE WHEN "
                               "a.reading + b.delta = b.reading THEN 1 ELSE 0 "
                               "END) AS closed FROM gauge a JOIN gauge b "
                               "ON b.run_id = a.run_id AND b.kind = a.kind "
                               "AND b.heap = a.heap AND b.id = (SELECT min(c.id) "
                               "FROM gauge c WHERE c.run_id = a.run_id "
                               "AND c.kind = a.kind AND c.heap = a.heap "
                               "AND c.id > a.id) WHERE a.run_id = " latest-run
                               " AND a.heap = 'runner'")) 0)
        pairs (* (length gauges) 2)]
    (assert (= row:pairs pairs)
            (string "the chain wants " pairs " consecutive pairs, got " row))
    (assert (= row:closed pairs)
            (string "every pair must chain; a break is a window charged to "
                    "nobody: " row)))

  # The worker heap a test-heap row measured is gone.
  (eprintln "scenario: a test-heap row carries no reading")
  (let [test (get (rows corpus:db
                        (string "SELECT count(*) AS n, count(reading) AS readings "
                                "FROM gauge WHERE heap = 'test' AND run_id = "
                                latest-run)) 0)]
    (assert (and (= test:n (* (length gauges) 3)) (= test:readings 0))
            (string "every file wants a test-heap row per gauge, each with no "
                    "reading: " test)))

  # A suite pass prints one summary per batch, so a block per heap on every
  # batch buried the lines that report a problem (docs/testing.md).
  (eprintln "scenario: no summary prints a gauge, and the store still holds them")
  (let [again ((elle-test corpus:db ["--summary"]) :err)]
    (each [what said] [["the run" corpus:err] ["--summary" again]]
      (each block ["runner heap" "test heap"]
        (assert (not (string/contains? said block))
                (string what " prints a " block " block: " said)))
      (each kind gauges
        (assert (not (string/contains? said (string " " kind " ")))
                (string what " prints the " kind " gauge: " said)))))

  # The trap: an array of constants is built once when the unit starts, and no
  # run would adopt it. This array holds the container's length, so each run
  # builds its own.
  (eprintln "scenario: a form that adopts charges the adoption to the test heap")
  (let [r (run-files "adopts"
                     [["adopts.lisp"
                       "(let [c (@array)]\n  (push c (array (length c) 2))\n  (length c))\n"]]
                     [] true)
        delta (fn [kind]
                (scalar r:db
                        (string "SELECT coalesce(sum(delta), 0) AS c FROM gauge "
                                "WHERE run_id = " latest-run
                                " AND heap = 'test' " "AND kind = '" kind
                                "' AND file LIKE '%adopts.lisp'")))
        adopts (delta "adopts")]
    (assert (>= adopts 1)
            (string "each run of the form adopts once, got " adopts))
    (assert (= adopts
               (+ (delta "owned-frees") (delta "rescues") (delta "extracts")
                  (delta "owned")))
            "adopts equals owned-frees, rescues, extracts and owned over the file's runs"))

  # No worker runs an isolated file. Its child's own report is child-gauges.lisp's.
  (eprintln "scenario: an isolated file records no test-heap rows")
  (let [r (run-files "isolated" [["iso.lisp" "(assert true \"ok\")\n"]]
                     ["--isolate" ""] true)
        got (heap-rows r:db)]
    (assert (and (= got:runner (length gauges)) (= got:test 0))
            (string "an isolated file wants a runner row per gauge and no test "
                    "row: " got)))

  # The counter-factual: a runner that took a gate for a form that never ran
  # would drop these readings, and the file would read as never measured.
  (eprintln "scenario: a file that gates in its shared setup records its test-heap rows")
  (let [r (run-files "gated"
                     [["gated.lisp"
                       (string "(def ok (gate! false \"an absent dependency\" true))\n"
                               "(assert ok \"never reached\")\n")]] [] true)
        statuses (this-run r:db "status FROM result")]
    (assert (and (not (empty? statuses))
                 (all? (fn [row] (= row:status "skip")) statuses))
            (string "the gate skips the whole-file form under every policy: "
                    statuses))
    (one-row-per-gauge r:db "test" 1 "the gated file"))

  # A file that fails to compile runs no form, so no worker read its heap.
  (eprintln "scenario: a file that fails to compile records no test-heap rows")
  (let [r (run-files "uncompiled"
                     [["uncompiled.lisp"
                       "(bound-nowhere 1)\n(assert true \"never reached\")\n"]]
                     [] false)
        got (heap-rows r:db)]
    (assert (and (= got:runner (length gauges)) (= got:test 0))
            (string "a file that fails to compile wants runner rows and no "
                    "test row: " got)))

  (eprintln "all gauges scenarios passed"))
