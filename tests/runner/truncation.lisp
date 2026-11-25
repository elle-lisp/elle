(elle/epoch 18)
## audited: 2026-11-22
## A run that did not finish reads as killed or as still running, never as green.
## tests/runner/overview.md
## docs/test-runner.md
##
## The OOM killer is the real kill: the whole corpus in one process can exceed
## the machine. The fixtures here SIGKILL the runner from inside a form, which
## does the same thing every time.
##
## The trap: a live run and a killed run leave the same row, `finished_at`
## NULL. Two batches sharing one session DB each warned that the other was
## killed, on every batch, while both were healthy. Naming the live one instead
## still drew a line per batch, so the next run says nothing about a live
## predecessor, and only `--summary` names it as running.

(include-file "lib/harness.lisp")

(with-temp-dir dir
  (def passes (write-fixture dir "a-passes.lisp" "(assert true \"ok\")\n"))
  (def kills (write-fixture dir "b-kills.lisp" "(os/sig-raise :sigkill)\n"))
  (def never (write-fixture dir "c-never-runs.lisp" "(assert true \"ok\")\n"))
  (def sleeps (write-fixture dir "sleeps.lisp" "(ev/sleep 120)\n"))

  (defn summary [db]
    ((elle-test db ["--summary"]) :err))

  (defn live-run [db]
    "Start `elle test` over a form that sleeps far longer than any check here,
     and answer it once its run row is in DB. The row lands at insert, a moment
     after the process starts."
    (let [live (subprocess/exec elle
                                ["test" "--db" db "--timeout" "240000" sleeps]
                                {:stdin :null :stdout :null :stderr :null})]
      (def @waited 0)
      (while (and (< waited 600)
                  (not (and (file-exists? db)
                            (let [[ok? n] (protect
                                            (scalar db
                                              "SELECT count(*) AS c FROM run"))]
                              (and ok? (= n 1))))))
        (ev/sleep 0.1)
        (assign waited (+ waited 1)))
      (assert (< waited 600) "the live runner wrote no run row within 60 s")
      live))

  (defn stop [live]
    "Kill LIVE and reap it. A second stop finds its status recorded, and
     sends nothing."
    (when (= (subprocess/kill live :sigkill) :signaled) (subprocess/wait live)))

  (eprintln "scenario: a completed run stamps finished_at and its counters")
  (let [db (path/join dir "done.db")
        r (elle-test db [passes])
        row (get (rows db
                       (string "SELECT (finished_at IS NOT NULL) AS done, "
                               "n_selected AS sel, (n_pass > 0) AS haspass "
                               "FROM run WHERE id = " latest-run)) 0)]
    (assert (= r:exit 0) (string "a trivial run gates green; stderr: " r:err))
    (assert (= row:done 1) (string "a completed run stamps finished_at: " row))
    (assert (= row:sel 1)
            (string "the run row records the files it planned: " row))
    (assert (= row:haspass 1)
            (string "a completed run aggregates its counters: " row)))

  # --summary labels the killed run DID NOT COMPLETE, with the live partial
  # tally rather than the zero stored counters, and the next run warns too.
  (eprintln "scenario: a killed run reads as truncated, not green")
  (let [db (path/join dir "killed.db")
        r (elle-test db [passes kills never])]
    (assert (= r:exit -9)
            (string "the fixture must SIGKILL the runner; exit " r:exit
                    " stderr: " r:err))
    (let [s (summary db)]
      (assert (string/contains? s "DID NOT COMPLETE")
              (string "--summary must label the killed run truncated: " s))
      (assert (string/contains? s "of 3 selected")
              (string "--summary must say how much of the selection it reached: "
                      s)))
    (let [next (elle-test db [never])]
      (assert (= next:exit 0)
              (string "the follow-up run gates green; stderr: " next:err))
      (assert (string/contains? next:err "DID NOT COMPLETE")
              (string "the next run must warn that its predecessor was killed: "
                      next:err))))

  # The second half is the counter-factual for the first: a view that said
  # "running" of every unfinished row would hide the kill pinned above.
  (eprintln "scenario: a run in flight reads as running until it is killed")
  (let [db (path/join dir "live.db")
        live (live-run db)
        pid (subprocess/pid live)]
    (defer
      (stop live)
      (assert (= pid
                 (scalar db "SELECT pid AS c FROM run WHERE finished_at IS NULL"))
              (string "the run row must carry the runner's own pid " pid))
      (let [running (summary db)]
        (assert (and (string/contains? running "STILL RUNNING")
                     (string/contains? running (string pid)))
                (string "--summary must label a live run running and name its "
                        "pid: " running))
        (assert (not (string/contains? running "DID NOT COMPLETE"))
                (string "--summary must not call a live run killed: " running)))
      (stop live)
      (let [killed (summary db)]
        (assert (string/contains? killed "DID NOT COMPLETE")
                (string "once its process is gone the run reads as killed: "
                        killed)))))

  # Two runs share a session DB whenever two batches or two worktrees run at
  # once. Every batch of a suite pass but the first starts beside a live
  # sibling, so a line about each one was a line per batch in every CI log.
  (eprintln "scenario: the next run says nothing about a live predecessor")
  (let [db (path/join dir "sibling.db")
        live (live-run db)
        pid (subprocess/pid live)]
    (defer
      (stop live)
      (let [next (elle-test db [passes])]
        (assert (= next:exit 0)
                (string "a run beside a live sibling gates green; stderr: "
                        next:err))
        (assert (not (or (string/contains? next:err "still running")
                         (string/contains? next:err (string "pid " pid))))
                (string "the next run must say nothing about a live "
                        "predecessor: " next:err))
        (assert (not (or (string/contains? next:err "was killed")
                         (string/contains? next:err "DID NOT COMPLETE")))
                (string "the next run must not call a live predecessor "
                        "killed: " next:err)))))

  (eprintln "all truncation scenarios passed"))
