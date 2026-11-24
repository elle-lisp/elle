(elle/epoch 18)
## audited: 2026-11-23
## A multi-form file's result records its worker's compile: its wall and CPU time, and `compile` gauge rows; every other result leaves them out.
## tests/runner/overview.md
## docs/test-cost.md
## docs/test-gauges.md
##
## The counter-factual: a worker that times nothing leaves its compile inside
## the result's wall time, where no reading tells it from the run. A clock
## around the run instead still writes two plausible numbers per row, so one
## scenario runs a file whose compile outweighs its run, and requires the
## compile's CPU time to exceed the run's.

(include-file "lib/harness.lisp")

(def timed "compile_wall_us > 0 AND compile_cpu_us > 0")
(def untimed "compile_wall_us IS NULL AND compile_cpu_us IS NULL")
(def two-forms "(def x 1)\n(assert (= x 1) \"one\")\n")

(defn kinds [db heap]
  "The latest run's gauge kinds on HEAP, sorted and joined."
  (string/join (sort (map (fn [r] r:kind)
                          (rows db
                                (string "SELECT kind FROM gauge WHERE heap = '"
                                        heap "' AND run_id = " latest-run))))
               ","))

(defn compile-rows [db what]
  "Require one compile-heap row per gauge the runner reads. Each run holds one
   file, so the runner heap holds one row per gauge."
  (assert (= (kinds db "compile") (kinds db "runner"))
          (string what ": one compile row per gauge, got " (kinds db "compile"))))

(defn no-compile-rows [db what]
  (assert (= (kinds db "compile") "")
          (string what " writes no compile row, got " (kinds db "compile"))))

(defn column [db tier col]
  "COL of the latest run's row on TIER."
  (scalar db
          (string "SELECT " col " AS c FROM result WHERE run_id = " latest-run
                  " AND tier = '" tier "'")))

(with-temp-dir dir
  (defn run [name src extra]
    "Run SRC as NAME.lisp into a store of its own, and answer the result."
    (elle-test (path/join dir (string name ".db"))
               [(write-fixture dir (string name ".lisp") src) ;extra]))

  (defn green [name src extra]
    "Run SRC, require the gate green, and answer the store."
    (let [r (run name src extra)]
      (assert (= r:exit 0)
              (string name ": the run must gate green; stderr: " r:err))
      r:db))

  (def multi (green "multi" two-forms []))

  (eprintln "scenario: a multi-form file times its worker's compile on every policy's row")
  (assert (= (row-tiers (rows multi
                              (string "SELECT tier FROM result WHERE run_id = "
                                      latest-run))) (tier-list script-tiers))
          "a multi-form file records one row per JIT policy")
  (every-row multi timed "every policy's row times its worker's compile")
  (every-row multi "compile_wall_us <= (wall_ms + 1) * 1000"
             "the compile falls inside the row's wall time")

  # The compile answers a thunk, and the window closes with that thunk live.
  (eprintln "scenario: the worker reads the gauges around its compile")
  (compile-rows multi "a multi-form file")
  (let [objects (scalar multi
                        (string "SELECT delta AS c FROM gauge WHERE heap = 'compile' "
                                "AND kind = 'objects' AND run_id = " latest-run))]
    (assert (> objects 0)
            (string "the compile leaves its thunk live, got " objects " objects")))

  (eprintln "scenario: a single-form file's worker compiles nothing")
  (let [db (green "single" "(assert true \"ok\")\n" [])]
    (every-row db untimed "a single-form row leaves the compile columns NULL")
    (no-compile-rows db "a single-form file"))

  # Two thousand definitions compile in tens of milliseconds and run in
  # microseconds. With the JIT off, no compile of the JIT's lands in cpu_us.
  (eprintln "scenario: the compile's CPU time is its own, and not the run's")
  (let [big (string/join (map (fn [i] (string "(def a" i " (+ " i " 1))"))
                              (range 0 2000)) "\n")
        large (green "large" (string big "\n(assert (= a0 1) \"a0\")\n") [])]
    (each tier script-tiers
      (let [l (column large tier "compile_cpu_us")
            s (column multi tier "compile_cpu_us")]
        (assert (> l (* 5 s))
                (string tier ": a larger file's compile costs more CPU: small "
                        s " us, large " l " us"))))
    (let [compile (column large "vm" "compile_cpu_us")
          ran (column large "vm" "cpu_us")]
      (assert (> compile ran)
              (string "cpu_us leaves the compile out: the compile took " compile
                      " us, the run " ran " us"))))

  (eprintln "scenario: a script that falls back to the runner's process times its compile there")
  (let [db (path/join dir "orphan.db")
        r (elle-test db [(fixture "orphan.lisp")])]
    (assert (= r:exit 0) (string "the in-process run passes; stderr: " r:err))
    (every-row db timed "every policy's in-process row times its compile")
    (compile-rows db "a script run in the runner's process"))

  (eprintln "scenario: a timeout times no compile")
  (let [r (run "timeout" "(ev/sleep 5)\n(assert true \"never\")\n"
               ["--timeout" "500"])]
    (assert (not (= r:exit 0)) "a timeout must gate non-zero")
    (every-row r:db (string "status = 'timeout' AND " untimed)
               "every row is a timeout that handed back no compile")
    (no-compile-rows r:db "a timeout"))

  (eprintln "scenario: a file that does not compile times no worker compile")
  (let [r (run "broken" "(def x 1)\n(assert (no-such-binding) \"unbound\")\n" [])]
    (assert (not (= r:exit 0)) "a file that does not compile gates non-zero")
    (every-row r:db untimed "a file-level error leaves the compile columns NULL")
    (no-compile-rows r:db "a file that does not compile"))

  (eprintln "scenario: an isolated child times no compile")
  (let [db (green "isolated" two-forms ["--isolate" ""])]
    (every-row db untimed "the child's row leaves the compile columns NULL")
    (no-compile-rows db "an isolated file"))

  (eprintln "scenario: an import keeps the compile columns and rows")
  (let [here (path/join dir "here.db")
        r (elle-test here ["--import" multi])]
    (assert (= r:exit 0) (string "the import lands; stderr: " r:err))
    (every-row here timed "every imported row keeps its compile")
    (compile-rows here "the imported run"))

  # SQLite drops a column on request, which is how the older table is made.
  (eprintln "scenario: a store from before the compile columns imports, and reads NULL")
  (let [old (green "old" two-forms [])
        here (path/join dir "from-old.db")]
    (query! old "ALTER TABLE result DROP COLUMN compile_wall_us")
    (query! old "ALTER TABLE result DROP COLUMN compile_cpu_us")
    (let [r (elle-test here ["--import" old])]
      (assert (= r:exit 0) (string "the import lands; stderr: " r:err))
      (every-row here untimed "a row recorded before the columns reads NULL")))

  (eprintln "scenario: a store from before the compile columns gains them")
  (let [db (path/join dir "migrate.db")]
    (query! db "ALTER TABLE result DROP COLUMN compile_wall_us")
    (query! db "ALTER TABLE result DROP COLUMN compile_cpu_us")
    (let [r (elle-test db [(write-fixture dir "migrate.lisp" two-forms)])]
      (assert (= r:exit 0) (string "the run gates green; stderr: " r:err))
      (every-row db timed "the migrated store records the worker's compile")))

  (eprintln "all worker-compile scenarios passed"))
