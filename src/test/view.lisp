(elle/epoch 12)
# audited: 2026-09-20
## elle test — reading a run back: the tally, the problem list, the warning
## about a predecessor that never finished, and raw SQL.
## docs/test-store.md
##
## A fragment of one module (see store.lisp).
##
## A run is otherwise silent (the gate is the exit code, the record is the DB).
## These render it: print-summary after every run, --summary for an existing
## DB, --query for arbitrary SQL. All read the session DB; none re-run
## anything.

# One line per non-pass row: status, file:line, [tier], reason.
(defn print-problems [conn run-id]
  (each p in (sqlite:query conn
                           (string "SELECT f.file AS file, f.line AS line, r.tier AS tier, "
                                   "r.status AS status, r.reason AS reason "
                                   "FROM result r JOIN form f ON f.hash = r.form_hash "
                                   "WHERE r.run_id = ?1 "
                                   "AND r.status IN ('fail', 'diverge', 'timeout') "
                                   "ORDER BY r.status, f.file") [run-id])
    (eprintln "  " (get p :status) "  " (get p :file)
              (if (get p :line) (string ":" (get p :line)) "") "  ["
              (get p :tier) "]  " (if (get p :reason) (get p :reason) ""))))

# Count one status's result rows for a run — the LIVE tally, straight from
# `result`. The stored run counters are written only at completion, so any
# view that read them would report a killed run as "0 fail"; every reader
# (the summaries here, the end-of-run aggregation, the gate) counts live.
(defn count-status [conn run-id st]
  (get (get (sqlite:query conn
                          "SELECT count(*) AS c FROM result WHERE run_id = ?1 AND status = ?2"
                          [run-id st]) 0) :c))

# How many distinct files a run recorded any result for (the "how far did the
# killed run get" numerator).
(defn files-recorded [conn run-id]
  (get (get (sqlite:query conn
                          "SELECT count(DISTINCT f.file) AS c FROM result r JOIN form f ON f.hash = r.form_hash WHERE r.run_id = ?1"
                          [run-id]) 0) :c))

# The run's code state, for the views. `commit` is an SQLite keyword, so the
# commit column answers to `sha` here.
(defn run-meta [conn run-id]
  (get (sqlite:query conn
                     "SELECT (finished_at IS NULL) AS trunc, n_selected AS sel, git_commit AS sha, git_dirty AS dirty, worktree AS worktree FROM run WHERE id = ?1"
                     [run-id]) 0))

(defn short-commit [sha]
  "The first seven characters of a commit, or nil when the run named none."
  (if (= sha nil)
    nil
    (if (> (length sha) 7) (slice sha 0 7) sha)))

# A tally read out of a log says nothing until it says which code it describes,
# so the run line carries the commit and whether the tree was dirty. A run that
# ran outside a repository has no commit, and says the run number alone.
(defn commit-note [meta]
  (let [short (short-commit (get meta :sha))]
    (if short
      (string " · commit " short (if (= (get meta :dirty) 1) " (dirty)" ""))
      "")))

# One session DB serves every checkout on the box, so a warning about an
# unfinished run has to say whose run it found. A path that is not yours is a
# sibling checkout still running, not a kill.
(defn worktree-note [meta]
  (let [w (get meta :worktree)]
    (if w (string " (worktree " w ")") "")))

# ── the measurements a run recorded (docs/test-store.md § Measurements) ──
# A tally by verdict, then a line for each reading that is neither `closed` nor
# `growth`. Those two are the expected answers — a reclaimed shape and a
# declared growth probe — so listing them would bury the readings a reader acts
# on under a few hundred that say nothing happened. The rest is a query.
(defn measurement-tally [conn run-id]
  (sqlite:query conn
                "SELECT verdict AS verdict, count(*) AS n FROM measurement WHERE run_id = ?1 GROUP BY verdict ORDER BY verdict"
                [run-id]))

(defn render-tally [rows]
  (if (empty? rows)
    ""
    (let [r (first rows)
          one (string (get r :n) " " (get r :verdict))]
      (if (empty? (rest rows))
        one
        (string one " · " (render-tally (rest rows)))))))

(defn print-measurements [conn run-id]
  (let [tally (measurement-tally conn run-id)
        total (get (get (sqlite:query conn
                                      "SELECT count(*) AS c FROM measurement WHERE run_id = ?1"
                                      [run-id]) 0) :c)]
    (when (> total 0)
      (eprintln total " measurement" (if (= total 1) "" "s") " · "
                (render-tally tally))
      (each m in (sqlite:query conn
                               (string "SELECT f.file AS file, m.subject AS subject, "
                                       "m.axis AS axis, m.value AS value, m.unit AS unit, "
                                       "m.verdict AS verdict FROM measurement m "
                                       "JOIN result r ON r.id = m.result_id "
                                       "JOIN form f ON f.hash = r.form_hash "
                                       "WHERE m.run_id = ?1 "
                                       "AND m.verdict NOT IN ('closed', 'growth') "
                                       "ORDER BY m.verdict, m.subject") [run-id])
        (eprintln "  " (get m :verdict) "  " (get m :file) "  " (get m :subject)
                  "  " (get m :axis) "  " (get m :value) " " (get m :unit)))))
  nil)

# ── what the run cost the runner (docs/test-store.md § The runner's own gauges) ──
# A leak per compiled file used to reach us as an OOM kill and a batch size,
# with nothing naming the file. These lines are that number: the run's totals,
# then the files that grew the runner's heap most.

# How many files the growers list names.
(def gauge-top 5)

# The gauge the list is ranked by. A region is the unit the corpus leak was
# measured in, and the one a per-file leak moves first.
(def gauge-rank "regions")

(defn signed [n]
  "A delta with its sign always written, so a flat window reads as a
   measurement taken rather than as one missing."
  (if (< n 0) (string n) (string "+" n)))

# One row per file, one `sum(CASE …)` column per gauge named for that gauge.
# Built from the gauge list, so a gauge added there arrives here already.
(defn gauge-growers-sql []
  (string "SELECT file AS file, "
          (string/join (map (fn [k]
                              (string "sum(CASE WHEN kind = '" k
                                      "' THEN delta END) AS " k)) (gauge-kinds))
                       ", ")
          " FROM gauge WHERE run_id = ?1 GROUP BY file ORDER BY " gauge-rank
          " DESC LIMIT " gauge-top))

# The run's total on one gauge, over every file it processed.
(defn gauge-total [conn run-id kind]
  (get (get (sqlite:query conn
                          "SELECT coalesce(sum(delta), 0) AS d FROM gauge WHERE run_id = ?1 AND kind = ?2"
                          [run-id kind]) 0) :d))

(defn render-gauge-totals [conn run-id]
  (string/join (map (fn [k] (string k " " (signed (gauge-total conn run-id k))))
                    (gauge-kinds)) " · "))

# One grower's deltas, in gauge order. A gauge the pivot left NULL reads 0.
(defn render-gauge-row [row]
  (string/join (map (fn [k]
                      (let [v (get row (keyword k))]
                        (string k " " (signed (if (= v nil) 0 v)))))
                    (gauge-kinds)) "  "))

(defn print-gauges [conn run-id]
  (let [rows (sqlite:query conn (gauge-growers-sql) [run-id])]
    (when (not (empty? rows))
      (eprintln "runner heap · " (render-gauge-totals conn run-id))
      (each r in rows
        (eprintln "  " (render-gauge-row r) "  " (get r :file)))))
  nil)

# Tally line + the problem rows (only when there are any). Tallies are computed
# live (count-status); a run without finished_at was KILLED mid-flight (OOM,
# signal — docs/test-runner.md § Run honesty) and is labelled so, because a
# partial all-pass result set must never read as green. To stderr, so it never
# mingles with --query's stdout or a test's captured output.
(defn print-summary [conn run-id]
  (let [meta (run-meta conn run-id)  # The DB is a SESSION: it accumulates every run. Show which run this is of
        # how many, so the persistent history is visible (query `run` for the rest).
        nruns (get (get (sqlite:query conn "SELECT count(*) AS c FROM run") 0)
                   :c)
        np (count-status conn run-id :pass)
        nf (count-status conn run-id :fail)
        ns (count-status conn run-id :skip)
        nd (count-status conn run-id :diverge)
        nt (count-status conn run-id :timeout)
        bad (+ nf nd nt)]
    (eprintln "")
    (if (= (get meta :trunc) 1)
      (eprintln "run " run-id
                " DID NOT COMPLETE — killed after recording results for "
                (files-recorded conn run-id) " of "
                (let [sel (get meta :sel)]
                  (if sel sel "?"))
                " selected files; the tally below is partial, not green")
      nil)
    (eprintln "elle test · run " run-id " of " nruns (commit-note meta))
    (eprintln np " pass · " ns " skip · " nf " fail · " nd " diverge · " nt
              " timeout")
    (if (> bad 0)
      (begin
        (eprintln bad " problem" (if (= bad 1) "" "s")
                  " (query the DB for full detail):")
        (print-problems conn run-id))
      nil)
    (print-measurements conn run-id)
    (print-gauges conn run-id)))

# Gate honesty at startup: if this session DB's latest run never completed,
# say so before starting a new one — the killed process could not report
# anything itself, and its absence of failures must not read as green. The
# warning names the worktree that run ran in: every checkout on the box shares
# this DB, so the row may belong to a sibling that is still running.
(defn warn-if-truncated [conn]
  (let [rows (sqlite:query conn
                           "SELECT id AS id FROM run WHERE finished_at IS NULL AND id = (SELECT max(id) FROM run)")]
    (if (empty? rows)
      nil
      (let [rid (get (get rows 0) :id)]
        (eprintln "warning: the previous run in this session DB was killed"
                  (worktree-note (run-meta conn rid)) ":")
        (print-summary conn rid)))))

# --query SQL: run it, print each row, exit. --summary: the latest run's tally.
(defn run-query [conn sql]
  (each r in (sqlite:query conn sql)
    (println r)))

(defn latest-run-id [conn]
  (let [rows (sqlite:query conn "SELECT max(id) AS id FROM run")]
    (if (empty? rows) nil (get (get rows 0) :id))))

# ── promote: render an ad-hoc form's syntax into <corpus>/<name>.lisp ─
(defn do-promote [conn opts]
  (let [id (get (get opts :promote) 0)
        name (get (get opts :promote) 1)
        rows (sqlite:query conn
                           "SELECT src AS src FROM form WHERE hash = ?1 LIMIT 1"
                           [id])]
    (if (empty? rows)
      (begin
        (eprintln "promote: no form with id " id)
        (os/exit 1))
      (let [src (get (get rows 0) :src)
            dir (get opts :corpus)
            out (string dir "/" name ".lisp")]
        (file/mkdir-all dir)
        (spit out (string "(elle/epoch 10)\n" src "\n"))
        (sqlite:close conn)
        (os/exit 0)))))
