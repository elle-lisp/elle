(elle/epoch 12)
# audited: 2026-09-17
## elle test — the session store: where a run is kept, the schema it is kept
## in, what a run row says about the code it ran against, and the CAS.
## docs/test-store.md
##
## A fragment of one module: src/main.rs concatenates the src/test files in
## order and compiles the result as the `elle test` subcommand, so a run needs
## no source tree. Bindings resolve across the whole concatenation.

(def sqlite ((import "std/sqlite")))
(def compress ((import "std/compress")))

# ── where the store lives ────────────────────────────────────────────
# A variable names a directory only when it is set AND non-empty. An empty
# ELLE_CACHE means "no disk cache" to the rest of the binary, and an empty
# ELLE_STATE taken literally would name the filesystem root.
(defn env-dir [name]
  (let [v (sys/env name)]
    (if (and v (> (length v) 0)) v nil)))

# Run history is a record, not a cache: nothing regenerates a run, and on a
# box where ELLE_CACHE is a tmpfs a reboot erases every run ever recorded. So
# the store follows the state variables first and reaches the cache only when
# nothing else names a directory. `--db` overrides all of it.
# docs/test-store.md § Run history is state.
(defn state-dir []
  (or (env-dir "ELLE_STATE")
      (let [xdg (env-dir "XDG_STATE_HOME")]
        (if xdg (string xdg "/elle") nil))
      (let [home (env-dir "HOME")]
        (if home (string home "/.local/state/elle") nil)) (env-dir "ELLE_CACHE")
      "target"))

# ── schema (subset of docs/test-store.md the v1 runner populates) ─────
# ALTER a column into a table that predates it. On a fresh DB the CREATE
# already carries the column, so the ALTER fails with "duplicate column
# name" — that is the no-op path; protect swallows it.
(defn ensure-column [conn table col decl]
  (protect (sqlite:exec conn
                        (string "ALTER TABLE " table " ADD COLUMN " col " " decl)))
  nil)

# The code state a run row carries. Named once, because the CREATE above and
# the migration below and the INSERT in main all have to agree about it.
(def run-code-columns
  [["git_commit" "TEXT"] ["git_dirty" "INTEGER"] ["tree_hash" "TEXT"]
   ["worktree" "TEXT"] ["elle_version" "TEXT"] ["build_profile" "TEXT"]
   ["host" "TEXT"] ["argv" "TEXT"]])

(defn ensure-code-columns [conn cols]
  (if (empty? cols)
    nil
    (begin
      (ensure-column conn "run" (get (first cols) 0) (get (first cols) 1))
      (ensure-code-columns conn (rest cols)))))

(defn ensure-schema [conn]
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS run (id INTEGER PRIMARY KEY, started_at TEXT DEFAULT (datetime('now')), finished_at TEXT, tiers TEXT, selection TEXT, n_selected INTEGER, git_commit TEXT, git_dirty INTEGER, tree_hash TEXT, worktree TEXT, elle_version TEXT, build_profile TEXT, host TEXT, argv TEXT, n_pass INTEGER DEFAULT 0, n_fail INTEGER DEFAULT 0, n_skip INTEGER DEFAULT 0, n_diverge INTEGER DEFAULT 0, n_timeout INTEGER DEFAULT 0)")
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS form (hash TEXT PRIMARY KEY, origin TEXT, session TEXT, file TEXT, form_index INTEGER, line INTEGER, col INTEGER, label TEXT, src TEXT, caps TEXT, touches TEXT, signal TEXT)")
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS result (id INTEGER PRIMARY KEY, run_id INTEGER, form_hash TEXT, tier TEXT, status TEXT, reason TEXT, expected TEXT, actual TEXT, syntax TEXT, signal TEXT, wall_ms INTEGER, cpu_us INTEGER)")
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS asset (result_id INTEGER, kind TEXT, hash TEXT, size INTEGER, codec TEXT)")
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS changed_file (run_id INTEGER, path TEXT, status TEXT, blob_hash TEXT)")
  # Run honesty (docs/test-runner.md § Run honesty): finished_at is stamped
  # only when a run completes, so a NULL is the kill marker. Migrate session
  # DBs that predate the columns, then backfill the legacy rows (recognizable
  # by n_selected IS NULL — new rows always set it): one that reached its
  # final counter aggregation had completed, and one with no results at all is
  # assumed completed (a zero-result selection is legal; only "results present
  # but counters never aggregated" is evidence of a kill). started_at is the
  # best available stamp. New-schema rows are never backfilled — for them the
  # stamp is authoritative, however empty the run.
  (ensure-column conn "run" "finished_at" "TEXT")
  (ensure-column conn "run" "n_selected" "INTEGER")
  # A run recorded before the code-state columns existed keeps NULL for each
  # of them: the run happened, and nothing recorded what it ran against.
  (ensure-code-columns conn run-code-columns)
  (sqlite:exec conn
               "UPDATE run SET finished_at = started_at WHERE finished_at IS NULL AND n_selected IS NULL AND (n_pass + n_fail + n_skip + n_diverge + n_timeout) > 0")
  (sqlite:exec conn
               "UPDATE run SET finished_at = started_at WHERE finished_at IS NULL AND n_selected IS NULL AND id NOT IN (SELECT DISTINCT run_id FROM result)"))

# ── what this run ran against ────────────────────────────────────────
# One command's trimmed stdout, or nil when it cannot run or says nothing.
# Through `sh -c` so a pipeline is one call, and under `protect` so a box with
# no git records NULL rather than failing the run.
(defn capture-cmd [cmd]
  (let [[ok? proc] (protect (subprocess/exec "sh" ["-c" cmd]))]
    (if (not ok?)
      nil
      (let [[read-ok? out] (protect (string (port/read-all (get proc :stdout))))]
        (protect (subprocess/wait proc))
        (if read-ok?
          (let [t (string/trim out)]
            (if (> (length t) 0) t nil))
          nil)))))

# The identity of the working tree, in one hash: the HEAD tree, the porcelain
# status, and the diff from HEAD. Two runs share it when they ran against the
# same code, dirty working tree included — which a commit alone cannot say.
# `git hash-object` does the hashing, so a large diff never crosses into the
# runner.
(def tree-hash-cmd
  (string "{ git rev-parse 'HEAD^{tree}'; git status --porcelain; "
          "git diff HEAD --binary; } 2>/dev/null | git hash-object --stdin 2>/dev/null"))

# The code state and the machine this run ran on. Outside a repository the git
# fields are nil, which lands as SQL NULL: the run happened, and nothing names
# the code it ran against. The host and the build are facts about the box and
# the binary, so they are recorded either way.
(defn run-identity []
  (let [commit (capture-cmd "git rev-parse HEAD 2>/dev/null")]
    (struct :commit commit
            :dirty (if commit
                     (if (capture-cmd "git status --porcelain 2>/dev/null") 1 0)
                     nil) :tree (if commit (capture-cmd tree-hash-cmd) nil)
            :worktree (capture-cmd "git rev-parse --show-toplevel 2>/dev/null")
            :host (capture-cmd "uname -n") :version (elle/version)
            :profile (elle/build-profile)
            :argv (string/join (rest (sys/argv)) " "))))

# ── CAS: content-addressed artifact store (docs/test-runner.md § CAS) ─────
# Artifacts (the --dump bodies) live on disk under `cas-dir` (a sibling of the
# session DB), content-addressed and zstd-compressed; the DB stores only the
# hash/size/codec. Identical artifacts across forms, tiers, and runs dedup to
# one file. `cas-addr` reuses the SAME builtin hash the runner uses for form
# identity (64-bit, build-stable — all a disposable local cache needs; see the
# docs' v1 boundaries for the cross-machine upgrade path).
(defn cas-addr [content]
  (string (hash content)))

# Store CONTENT (a string) and return [addr size codec] for its asset row.
# `size` is the UNCOMPRESSED byte length (so SQL reasons about logical size and
# the codec can change without moving the artifact); the write is skipped when
# the addressed file already exists (dedup).
(defn cas-put [content]
  (let [addr (cas-addr content)
        path (string cas-dir "/" addr)
        size (length (bytes content))]
    (if (file-exists? path)
      nil
      (let [p (port/open-bytes path :write)]
        (port/write p (compress:zstd (bytes content)))
        (port/close p)))
    [addr size "zstd"]))

# --dump capture is OMITTED for now (docs/test-runner.md § CAS asset capture
# status note): the per-file (compile/dumps …) pass is the single largest
# contributor to the corpus region leak that OOMs `make smoke` (~28k regions/
# file), and the dumps are not byte-deterministic across compiles (absolute
# @-HirIds from a process-global counter), so they would not even CAS-dedup.
# Until that leak is root-caused and fixed, capture nothing: no compile/dumps
# call, no dump asset rows, no CAS dump files. Re-enabling is reverting this to
# the compile/dumps body below. stdout/stderr capture is a separate path
# (capture-stdio, on the per-form execution) and is unaffected.
#
# The original (re-enable here once the leak is fixed):
#   (let [out (protect (compile/dumps src name))]
#     (if (get out 0)
#       (let [d (get out 1)]
#         (filter (fn [x] (not (= x nil)))
#                 (map (fn [k]
#                        (let [text (get d k)]
#                          (if (and (not (= text nil)) (> (length text) 0))
#                            (concat [(string k)] (cas-put text))
#                            nil)))
#                      [:ast :fhir :defuse :regions :hir :lir :cfg :dfa :jit
#                       :escape])))
#       [])))
(defn capture-dumps [src name]
  [])

# Insert one asset row (dump = [kind addr size codec]) for a result.
(defn insert-asset [conn result-id dump]
  (sqlite:exec conn
               "INSERT INTO asset (result_id, kind, hash, size, codec) VALUES (?1,?2,?3,?4,?5)"
               [result-id (get dump 0) (get dump 1) (get dump 2) (get dump 3)]))

(defn insert-assets [conn result-id dumps]
  (if (empty? dumps)
    nil
    (begin
      (insert-asset conn result-id (first dumps))
      (insert-assets conn result-id (rest dumps)))))

# CAS-store a result's captured stdout/stderr (only when non-empty, so a silent
# test adds no asset rows) and record them as `stdout`/`stderr` assets.
(defn capture-stdio [conn result-id out-str err-str]
  (if (and (not (= out-str nil)) (> (length out-str) 0))
    (insert-asset conn result-id (concat ["stdout"] (cas-put out-str)))
    nil)
  (if (and (not (= err-str nil)) (> (length err-str) 0))
    (insert-asset conn result-id (concat ["stderr"] (cas-put err-str)))
    nil))
