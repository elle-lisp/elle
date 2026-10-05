(elle/epoch 14)
# audited: 2026-10-05
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
                     "SELECT (finished_at IS NULL) AS trunc, n_selected AS sel, git_commit AS sha, git_dirty AS dirty, worktree AS worktree, host AS host, pid AS pid FROM run WHERE id = ?1"
                     [run-id]) 0))

# An unfinished row is a run still in flight when its process is still alive:
# the row's host is this host, and `ps` shows an elle under the row's pid. The
# name check keeps a pid the kernel has since handed to another program from
# reading as the run. Anything else — another host, no pid, a pid that is gone
# — is a kill (docs/test-runner.md).
(defn run-alive? [meta]
  (let [pid (get meta :pid)
        host (get meta :host)]
    (if (or (= pid nil) (= host nil) (not (= host (capture-cmd "uname -n"))))
      false
      (let [comm (capture-cmd (string "ps -p " pid " -o comm= 2>/dev/null"))]
        (and (not (= comm nil)) (string/contains? comm "elle"))))))

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
# unfinished run has to say whose run it found.
(defn worktree-note [meta]
  (let [w (get meta :worktree)]
    (if w (string " (worktree " w ")") "")))

(defn running-note [meta]
  (let [w (get meta :worktree)]
    (string " (pid " (get meta :pid) (if w (string ", worktree " w) "") ")")))

# ── the readings a run recorded (docs/test-store.md § Measurements) ──
# A tally by verdict, then a line for each reading that is neither `ok` nor
# unjudged. Those two are the expected answers — a reading within its bound,
# and one with no row of the run's build yet — so listing them would bury the
# readings a reader acts on under a few hundred that say nothing happened. The
# rest is a query.

# The order the tally reads in: the expected answer first, then the gating
# verdicts as the judge names them, then the readings nothing judged.
(def verdict-order
  ["ok" "regression" "stale" "unledgered" "missing" "void" "unjudged"])

(defn measurement-tally [conn run-id]
  "Verdict → count for the run, with a NULL verdict under `unjudged`."
  (let [@t @{}]
    (each r in (sqlite:query conn
                             "SELECT verdict AS verdict, count(*) AS n FROM measurement WHERE run_id = ?1 GROUP BY verdict"
                             [run-id])
      (put t
           (let [v (get r :verdict)]
             (if v v "unjudged")) (get r :n)))
    t))

(defn render-tally [t]
  (string/join (map (fn [v] (string (get t v) " " v))
                    (filter (fn [v] (get t v)) verdict-order)) " · "))

# The listing's order is the tally's: `ORDER BY` a CASE over `verdict-order`,
# so every regression is read before the first stale row.
(defn verdict-rank-sql []
  (string "CASE m.verdict "
          (string/join (map (fn [i]
                              (string "WHEN '" (get verdict-order i) "' THEN " i))
                            (->list (range (length verdict-order)))) " ") " END"))

# One reading's line: the verdict, the file and the tier, the subject and the
# axis, then the reading with its half-width and unit, then the bound it met.
# A missing row has no reading, so it ends at the axis.
(defn render-measurement [m]
  (string (get m :verdict) "  " (get m :file) "  [" (get m :tier) "]  "
          (get m :subject) "  " (get m :axis)
          (if (= (get m :value) nil)
            ""
            (let [k (get m :kind)]
              (string "  " (get m :value) " ±" (get m :half) " " (get m :unit)
                      "  "
                      (ledger:describe-bound (if k (keyword k) nil)
                      (get m :bound)))))))

(defn print-measurements [conn run-id]
  (let [tally (measurement-tally conn run-id)
        total (get (get (sqlite:query conn
                                      "SELECT count(*) AS c FROM measurement WHERE run_id = ?1"
                                      [run-id]) 0) :c)]
    (when (> total 0)
      (eprintln total " reading" (if (= total 1) "" "s") " · "
                (render-tally tally))
      (each m in (sqlite:query conn
                               (string "SELECT f.file AS file, r.tier AS tier, "
                                       "m.subject AS subject, m.axis AS axis, "
                                       "m.value AS value, m.half AS half, "
                                       "m.unit AS unit, m.bound AS bound, m.kind AS kind, "
                                       "m.verdict AS verdict FROM measurement m "
                                       "JOIN result r ON r.id = m.result_id "
                                       "JOIN form f ON f.hash = r.form_hash "
                                       "WHERE m.run_id = ?1 "
                                       "AND m.verdict IS NOT NULL AND m.verdict != 'ok' "
                                       "ORDER BY " (verdict-rank-sql)
                                       ", f.file, r.tier, m.subject") [run-id])
        (eprintln "  " (render-measurement m))))
    # A run with no build left its readings unrecorded, so a green run that
    # printed some must not read as a passed gate (docs/ratchet.md).
    (when (and (= total 0) (> unrecorded-readings 0))
      (eprintln unrecorded-readings " reading"
                (if (= unrecorded-readings 1) "" "s")
                " printed · no build, so none recorded or judged: run under elle-rig test")))
  nil)

# ── what the run cost each heap (docs/test-gauges.md) ─────────────────
# A leak per compiled file used to reach us as an OOM kill and a batch size,
# with nothing naming the file. These lines are that number, one block per
# heap: the run's totals, then the files that grew that heap most.

# How many files each growers list names.
(def gauge-top 5)

# The gauge the lists are ranked by. A region is the unit the corpus leak was
# measured in, and the one a per-file leak moves first.
(def gauge-rank "regions")

# The heaps a run records, in the order the summary prints them.
(def gauge-heaps ["runner" "test"])

# A row's heap. A store written before the heap column existed holds runner
# rows alone, and they read NULL there.
(def gauge-heap-sql "coalesce(heap, 'runner')")

(defn signed [n]
  "A delta with its sign always written, so a flat window reads as a
   measurement taken rather than as one missing."
  (if (< n 0) (string n) (string "+" n)))

# One row per file of one heap, one `sum(CASE …)` column per gauge named for
# that gauge. Built from the gauge list, so a gauge added there arrives here
# already. The names are quoted because a gauge name carries hyphens.
(defn gauge-growers-sql []
  (string "SELECT file AS file, "
          (string/join (map (fn [k]
                              (string "sum(CASE WHEN kind = '" k
                                      "' THEN delta END) AS \"" k "\""))
                            (gauge-kinds)) ", ")
          " FROM gauge WHERE run_id = ?1 AND " gauge-heap-sql
          " = ?2 GROUP BY file ORDER BY \"" gauge-rank "\" DESC LIMIT "
          gauge-top))

# The run's total on one gauge of one heap, over every file it processed.
(defn gauge-total [conn run-id heap kind]
  (get (get (sqlite:query conn
                          (string "SELECT coalesce(sum(delta), 0) AS d FROM gauge WHERE run_id = ?1 AND "
                                  gauge-heap-sql " = ?2 AND kind = ?3")
                          [run-id heap kind]) 0) :d))

(defn render-gauge-totals [conn run-id heap]
  (string/join (map (fn [k]
                      (string k " " (signed (gauge-total conn run-id heap k))))
                    (gauge-kinds)) " · "))

# One grower's deltas, in gauge order. A gauge the pivot left NULL reads 0.
(defn render-gauge-row [row]
  (string/join (map (fn [k]
                      (let [v (get row (keyword k))]
                        (string k " " (signed (if (= v nil) 0 v)))))
                    (gauge-kinds)) "  "))

(defn print-gauges [conn run-id]
  (each heap in gauge-heaps
    (let [rows (sqlite:query conn (gauge-growers-sql) [run-id heap])]
      (when (not (empty? rows))
        (eprintln heap " heap · " (render-gauge-totals conn run-id heap))
        (each r in rows
          (eprintln "  " (render-gauge-row r) "  " (get r :file))))))
  nil)

# Tally line + the problem rows (only when there are any). Tallies are computed
# live (count-status). A run without finished_at is either still running or was
# KILLED mid-flight (OOM, signal — docs/test-runner.md), and is labelled as
# whichever it is, because a partial all-pass result set must never read as
# green. To stderr, so it never mingles with --query's stdout or a test's
# captured output.
(defn print-summary [conn run-id]
  (let [meta (run-meta conn run-id)
        # The DB is a SESSION: it accumulates every run. Show which run this is of
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
      (let [sel (let [n (get meta :sel)]
                  (if n n "?"))
            done (files-recorded conn run-id)]
        (if (run-alive? meta)
          (eprintln "run " run-id " STILL RUNNING (pid " (get meta :pid)
                    ") — results for " done " of " sel
                    " selected files so far; the tally below is partial, not green")
          (eprintln "run " run-id
                    " DID NOT COMPLETE — killed after recording results for "
                    done " of " sel
                    " selected files; the tally below is partial, not green")))
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
# anything itself, and its absence of failures must not read as green. Every
# checkout on the box shares this DB, and two runs may share it at once, so a
# row still being written by a live sibling gets one line naming it, not the
# kill warning.
(defn warn-if-truncated [conn]
  (let [rows (sqlite:query conn
                           "SELECT id AS id FROM run WHERE finished_at IS NULL AND id = (SELECT max(id) FROM run)")]
    (if (empty? rows)
      nil
      (let [rid (get (get rows 0) :id)
            meta (run-meta conn rid)]
        (if (run-alive? meta)
          (eprintln "note: the previous run in this session DB is still running"
                    (running-note meta))
          (begin
            (eprintln "warning: the previous run in this session DB was killed"
                      (worktree-note meta) ":")
            (print-summary conn rid)))))))

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
