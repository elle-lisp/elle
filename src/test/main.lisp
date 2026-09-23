(elle/epoch 12)
# audited: 2026-09-22
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

# Every flag the runner takes, as [key how-it-reads]. Written as data because
# the alternative is a stack of `if`s one level deeper per flag, where adding
# one re-indents every flag under it and the diff hides which one was added.
#
# How a flag reads is the whole vocabulary: `:flag` takes nothing, `:value` the
# next argument, `:int` the next argument as a number, `:append` the next
# argument onto a list, `:pair` the next two. Anything not named here is a
# path.
(def flag-spec
  @{"--db" [:db :value]
    "--timeout" [:timeout :int]
    "--wide" [:wide :append]
    "--wide-timeout" [:wide-timeout :int]
    "--budget" [:budget :flag]
    "--isolate" [:isolate :value]
    "--corpus" [:corpus :value]
    "--reset" [:reset :flag]
    "--import" [:import :value]
    "--query" [:query :value]
    "--summary" [:summary :flag]
    "-e" [:eval :append]
    "--promote" [:promote :pair]})

# How many arguments a flag consumes after itself.
(defn flag-width [how]
  (case how
    :flag 0
    :pair 2
    1))

# Record one flag's value under `key` and answer nothing; `rest-args` is what
# follows the flag itself.
(defn apply-flag [acc key how rest-args]
  (case how
    :flag (put acc key true)
    :value (put acc key (first rest-args))
    :int
      (put acc key (parse-int (first rest-args)))
    :append
      (put acc key (concat (get acc key) [(first rest-args)]))
    :pair
      (put acc key [(first rest-args) (first (rest rest-args))]))
  nil)

(defn drop-n [xs n]
  (if (or (= n 0) (empty? xs)) xs (drop-n (rest xs) (- n 1))))

(defn parse-args [args acc]
  (if (empty? args)
    acc
    (let [a (first args)
          spec (get flag-spec a)]
      (if (= spec nil)
        (begin
          (put acc :paths (concat (get acc :paths) [a]))
          (parse-args (rest args) acc))
        (let [how (get spec 1)]
          (apply-flag acc (get spec 0) how (rest args))
          (parse-args (drop-n (rest args) (flag-width how)) acc))))))

# ── main ─────────────────────────────────────────────────────────────
(def opts
  (parse-args (drop-sep (rest (sys/argv)))
              @{:db nil
                :corpus "tests"
                :reset false
                :timeout 60000
                :wide []
                :wide-timeout nil
                :budget false
                :eval []
                :isolate nil
                :promote nil
                :import nil
                :query nil
                :summary false
                :paths []}))

# `--isolate FLAGS` runs each path as its own child, `elle FLAGS PATH`, for a
# mode the process sets once and no worker thread can vary
# (docs/test-runner.md § Isolation). The flag string may be empty; nil here is
# the ordinary in-process run.
(def isolate-flags (get opts :isolate))

# An ad-hoc form has no file, and a child process is given a path. Refuse the
# combination up front, rather than running part of the selection in-process
# and leaving the run to read as though one mode covered all of it.
(if (and isolate-flags (not (empty? (get opts :eval))))
  (begin
    (eprintln "elle test: --isolate runs a path in its own process, and -e has no file to give one")
    (os/exit 2))
  nil)

# The child is this binary, so a box whose OS will not name the running
# executable cannot have this mode. Say so here: the alternative is every path
# failing on a spawn with no program, which reads as the corpus being broken.
(if (and isolate-flags (= (elle/executable) nil))
  (begin
    (eprintln "elle test: --isolate needs the path of this binary, and the OS did not give one")
    (os/exit 2))
  nil)

# The default per-form wall-clock budget (ms), which every path takes unless it
# earns the wider one below. A test form whose worker does not finish within its
# budget is recorded `timeout` (not fail/pass), and the run gates non-zero.
# os/join yields to the scheduler while waiting (no polling); on the deadline it
# raises {:error :timeout} and the runaway worker is abandoned (see § Isolation).
(def test-timeout-ms (get opts :timeout))

# A budget follows the FILE a form came from, not the run. Some corpus families
# carry a `deadline` of their own that is wider than the default budget, and a
# form killed at the narrower one never prints the report that deadline exists
# to give. `--wide` names a path substring and `--wide-timeout` is what a named
# path's forms get; the Makefile owns the list of families and hands it to every
# pass, so no second copy of it lives here (docs/test-cli.md).
(def wide-patterns (get opts :wide))
(def wide-timeout-ms (or (get opts :wide-timeout) test-timeout-ms))

(defn wide-path? [path]
  (not (empty? (filter (fn [p] (string/contains? path p)) wide-patterns))))

(defn budget-for [path]
  (if (wide-path? path) wide-timeout-ms test-timeout-ms))

# `--budget` answers and exits, ahead of the store: the answer follows from the
# flags alone, and a query must leave no run row behind.
(if (get opts :budget)
  (begin
    (each f in (get opts :paths)
      (println (budget-for f) " " f))
    (os/exit 0))
  nil)

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

# --import merges another store's runs into this one and exits. It records no
# run of its own: nothing ran here (docs/test-store.md).
(if (get opts :import)
  (begin
    (do-import conn (get opts :import))
    (sqlite:close conn)
    (os/exit 0))
  nil)

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
             "INSERT INTO run (tiers, n_selected, git_commit, git_dirty, tree_hash, worktree, boot_fingerprint, elle_version, build_profile, host, argv, run_key) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)"
             [(if isolate-flags "process" (tiers-str active-tiers))
              (+ (length (get opts :paths)) (length (get opts :eval)))
              (get ident :commit) (get ident :dirty) (get ident :tree)
              (get ident :worktree) (get ident :boot) (get ident :version)
              (get ident :profile) (get ident :host) (get ident :argv)
              (get ident :key)])
(def run-id (last-rowid conn))

# What the runner's own heap reads before the first file. Every later reading
# is taken at a file boundary and charged to the file that boundary closes
# (docs/test-store.md § The runner's own gauges).
(def gauge-prev (gauge-baseline))

# Run every file/eval for its side effect: each writes its result rows to the DB.
# We do NOT aggregate the returned status lists in memory — for a large corpus
# that built a list one recursive `flat` per file deep and blew the VM's call
# depth. The tally is a GROUP BY over the rows we just wrote (the DB is the
# source of truth anyway), so it is independent of corpus size.
(each f in (get opts :paths)
  (parameterize ((*form-budget-ms* (budget-for f)))
    (if isolate-flags
      (process-file-isolated conn run-id f isolate-flags)
      (process-file conn run-id f)))
  (gauge-mark conn run-id gauge-prev f))
(each e in (get opts :eval)
  (process-eval conn run-id e)
  (gauge-mark conn run-id gauge-prev "<eval>"))

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
