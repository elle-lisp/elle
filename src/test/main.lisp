(elle/epoch 12)
# audited: 2026-09-17
## elle test — the command line, the store it opens, and the run it drives.
## docs/test-cli.md
##
## A fragment of one module (see store.lisp), and the last one: everything
## below the definitions runs the invocation.

# ── argv ─────────────────────────────────────────────────────────────
# (rest (sys/argv)) drops the program name; tolerate a leading "--" so an
# argument list that arrives behind a separator reads the same as `elle test ...`.
(defn drop-sep [args]
  (if (and (not (empty? args)) (= (first args) "--")) (rest args) args))

(defn parse-args [args acc]
  (if (empty? args)
    acc
    (let [a (first args)]
      (if (= a "--db")
        (begin
          (put acc :db (first (rest args)))
          (parse-args (rest (rest args)) acc))
        (if (= a "--timeout")
          (begin
            (put acc :timeout (parse-int (first (rest args))))
            (parse-args (rest (rest args)) acc))
          (if (= a "--corpus")
            (begin
              (put acc :corpus (first (rest args)))
              (parse-args (rest (rest args)) acc))
            (if (= a "--reset")
              (begin
                (put acc :reset true)
                (parse-args (rest args) acc))
              (if (= a "--query")
                (begin
                  (put acc :query (first (rest args)))
                  (parse-args (rest (rest args)) acc))
                (if (= a "--summary")
                  (begin
                    (put acc :summary true)
                    (parse-args (rest args) acc))
                  (if (= a "-e")
                    (begin
                      (put acc
                           :eval (concat (get acc :eval) [(first (rest args))]))
                      (parse-args (rest (rest args)) acc))
                    (if (= a "--promote")
                      (begin
                        (put acc
                             :promote [(first (rest args))
                                       (first (rest (rest args)))])
                        (parse-args (rest (rest (rest args))) acc))
                      (begin
                        (put acc :paths (concat (get acc :paths) [a]))
                        (parse-args (rest args) acc)))))))))))))

# ── main ─────────────────────────────────────────────────────────────
(def opts
  (parse-args (drop-sep (rest (sys/argv)))
              @{:db nil
                :corpus "tests"
                :reset false
                :timeout 60000
                :eval []
                :promote nil
                :query nil
                :summary false
                :paths []}))

# Per-test wall-clock budget (ms). A test form whose worker does not finish
# within it is recorded `timeout` (not fail/pass), and the run gates non-zero.
# os/join yields to the scheduler while waiting (no polling); on the deadline it
# raises {:error :timeout} and the runaway worker is abandoned (see § Isolation).
(def test-timeout-ms (get opts :timeout))

# `--db` names the store outright; otherwise it is the state directory, which
# survives a reboot (docs/test-store.md § Run history is state).
(def db (or (get opts :db) (string (state-dir) "/elle-tests.db")))

# The CAS lives beside the session DB: <db-dir>/cas/<hash>. Created up front so
# cas-put can write into it. `scratch-dir` holds the per-(form × tier)
# stdout/stderr redirect files (written + slurped + deleted by each worker; the
# directory is disposable).
(def cas-dir (string (path/parent db) "/cas"))
(def scratch-dir (string (path/parent db) "/scratch"))

(if (get opts :reset)
  (begin
    (if (file-exists? db) (file/delete db) nil)
    (if (file-exists? cas-dir) (file/delete-dir cas-dir) nil)
    (if (file-exists? scratch-dir) (file/delete-dir scratch-dir) nil)
    (os/exit 0))
  nil)

(file/mkdir-all cas-dir)
(file/mkdir-all scratch-dir)

(def conn (sqlite:open db))
(ensure-schema conn)

(if (get opts :promote) (do-promote conn opts) nil)

# --query / --summary: inspect an existing session DB and exit (never re-run).
(if (get opts :query)
  (begin
    (run-query conn (get opts :query))
    (sqlite:close conn)
    (os/exit 0))
  nil)
(if (get opts :summary)
  (let [rid (latest-run-id conn)]
    (if rid (print-summary conn rid) (eprintln "elle test: no runs in " db))
    (sqlite:close conn)
    (os/exit 0))
  nil)

(warn-if-truncated conn)

# n_selected and the code state land at insert (everything else about the row
# is written at completion), so a killed run's row still says how much work was
# planned and which commit, worktree, and machine it was planned on.
(def ident (run-identity))
(sqlite:exec conn
             "INSERT INTO run (tiers, n_selected, git_commit, git_dirty, tree_hash, worktree, elle_version, build_profile, host, argv) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)"
             [(tiers-str active-tiers)
              (+ (length (get opts :paths)) (length (get opts :eval)))
              (get ident :commit) (get ident :dirty) (get ident :tree)
              (get ident :worktree) (get ident :version) (get ident :profile)
              (get ident :host) (get ident :argv)])
(def run-id
  (get (get (sqlite:query conn "SELECT last_insert_rowid() AS id") 0) :id))

# Run every file/eval for its side effect: each writes its result rows to the DB.
# We do NOT aggregate the returned status lists in memory — for a large corpus
# that built a list one recursive `flat` per file deep and blew the VM's call
# depth. The tally is a GROUP BY over the rows we just wrote (the DB is the
# source of truth anyway), so it is independent of corpus size.
(each f in (get opts :paths)
  (process-file conn run-id f))
(each e in (get opts :eval)
  (process-eval conn run-id e))

(def nfail (count-status conn run-id :fail))
(def npass (count-status conn run-id :pass))
(def nskip (count-status conn run-id :skip))
(def ndiverge (count-status conn run-id :diverge))
(def ntimeout (count-status conn run-id :timeout))

# Counters and finished_at land in ONE statement: the completion stamp. A run
# row without it was killed mid-flight and reads as truncated everywhere
# (docs/test-runner.md § Run honesty).
(sqlite:exec conn
             "UPDATE run SET n_pass = ?1, n_fail = ?2, n_skip = ?3, n_diverge = ?4, n_timeout = ?5, finished_at = datetime('now') WHERE id = ?6"
             [npass nfail nskip ndiverge ntimeout run-id])
# Always render the run: the tally, plus every problem row with its reason — so
# you read results here, not by hand-writing SQLite (use --query to drill in).
(print-summary conn run-id)
(sqlite:close conn)
# Gate exit: zero iff no form failed, no tier diverged, and nothing timed out.
# A skip is fine; a timeout (a test that never finished) gates non-zero.
(os/exit (if (or (> nfail 0) (> ndiverge 0) (> ntimeout 0)) 1 0))
