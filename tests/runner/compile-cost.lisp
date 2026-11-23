(elle/epoch 18)
## audited: 2026-11-23
## Each compile the runner runs on its own thread writes a compile_cost row: the file, the step, and its wall and CPU time.
## tests/runner/overview.md
## docs/test-store.md
##
## The counter-factual: a timer around the wrong call, the row's own INSERT
## for one, still writes two plausible numbers per file. So one scenario
## compiles a large file beside a small one, and requires the large file's
## module compile to cost several times the small one's CPU.

(include-file "lib/harness.lisp")

(with-temp-dir dir
  (defn steps [db file]
    "The latest run's compile_cost rows for FILE, as step → [wall_us cpu_us]."
    (let [@out @{}]
      (each r in (rows db
                       (string "SELECT step, wall_us, cpu_us FROM compile_cost "
                               "WHERE run_id = " latest-run " AND file LIKE '%"
                               file "'"))
        (assert (nil? (get out r:step))
                (string file " records its " r:step " compile once"))
        (put out r:step [r:wall_us r:cpu_us]))
      out))

  (defn timed? [cost]
    "Whether COST is a wall and a CPU time, each above zero."
    (and cost (> (get cost 0) 0) (> (get cost 1) 0)))

  (defn run [name src extra]
    "Run SRC as NAME.lisp into a store of its own, and answer the result."
    (elle-test (path/join dir (string name ".db"))
               [(write-fixture dir (string name ".lisp") src) ;extra]))

  (defn both-steps [name src what]
    "Run SRC, and require its file to record a timed profile and module
     compile. Answer the run's store."
    (let [r (run name src [])
          s (steps r:db (string name ".lisp"))]
      (assert (timed? (get s "profile"))
              (string what ": the profile analysis is timed, got " s))
      (assert (timed? (get s "module"))
              (string what ": the module compile is timed, got " s))
      (assert (= (length (keys s)) 2)
              (string what ": two steps and no other, got " s))
      r:db))

  (eprintln "scenario: a single-form file times its analysis and its module compile")
  (both-steps "single" "(assert true \"ok\")\n" "a single-form file")

  (eprintln "scenario: a multi-form file times them too")
  (both-steps "multi" "(def x 1)\n(assert (= x 1) \"one\")\n"
              "a multi-form file")

  (eprintln "scenario: a file that does not compile keeps both rows")
  (both-steps "broken" "(def x 1)\n(assert (no-such-binding) \"unbound\")\n"
              "a file that does not compile")

  (eprintln "scenario: a path the runner cannot read compiles nothing")
  (let [db (path/join dir "absent.db")
        r (elle-test db [(path/join dir "no-such-file.lisp")])]
    (assert (not (= r:exit 0)) "an unreadable path gates red")
    (assert (= (scalar db "SELECT count(*) AS c FROM compile_cost") 0)
            "an unreadable path writes no compile_cost row"))

  (eprintln "scenario: under --isolate the runner times its analysis alone")
  (let [r (run "isolated" "(assert true \"ok\")\n" ["--isolate" ""])
        s (steps r:db "isolated.lisp")]
    (assert (= r:exit 0) (string "the child must pass; stderr: " r:err))
    (assert (timed? (get s "profile")) (string "the analysis is timed, got " s))
    (assert (nil? (get s "module"))
            (string "the child compiles the module, not the runner, got " s)))

  # Two thousand definitions compile in milliseconds, and two in microseconds.
  (eprintln "scenario: a larger file's module compile costs more CPU")
  (let [big (string/join (map (fn [i] (string "(def a" i " (+ " i " 1))"))
                              (range 0 2000)) "\n")
        small (steps (run "small" "(def a 1)\n(assert (= a 1) \"a\")\n" [])
                     "small.lisp")
        large (steps (run "large" (string big "\n(assert (= a0 1) \"a0\")\n") [])
                     "large.lisp")
        cpu (fn [s] (get (get s "module") 1))]
    (assert (> (cpu large) (* 5 (cpu small)))
            (string "the module row must time the compile: small " (cpu small)
                    " us, large " (cpu large) " us")))

  (eprintln "scenario: an import keeps the rows, and a store without the table imports")
  (let [db (both-steps "far" "(assert true \"ok\")\n" "the imported file")
        here (path/join dir "here.db")
        old (path/join dir "old.db")]
    (assert (= ((elle-test here ["--import" db]) :exit) 0) "the import lands")
    (assert (timed? (get (steps here "far.lisp") "module"))
            "the imported store keeps the module row")
    (let [r (run "old" "(assert true \"ok\")\n" [])]
      (query! r:db "DROP TABLE compile_cost")
      (let [i (elle-test old ["--import" r:db])]
        (assert (= i:exit 0)
                (string "a store from before compile_cost imports; stderr: "
                        i:err)))))

  (eprintln "scenario: a store from before the table gains it")
  (let [db (path/join dir "migrate.db")]
    (query! db "DROP TABLE compile_cost")
    (let [r (elle-test db
                       [(write-fixture dir "migrate.lisp"
                                       "(assert true \"ok\")\n")])]
      (assert (= r:exit 0) (string "the run gates green; stderr: " r:err))
      (assert (timed? (get (steps db "migrate.lisp") "module"))
              "the migrated store records the module compile")))

  (eprintln "all compile-cost scenarios passed"))
