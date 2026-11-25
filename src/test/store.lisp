(elle/epoch 18)
# audited: 2026-11-24
## elle test — the session store: where a run is kept, its schema, the code
## state a run row records, and the CAS.
## docs/test-store.md
##
## A fragment of one module: src/program/subcommand.rs concatenates the
## src/test files in order and compiles the result as the `elle test`
## subcommand, so a run needs no source tree. A name exists from its def
## onward, so a fragment reads what it or an earlier fragment defines: this
## file, then gauge, ledger, import, deadline, exec, tiers, child, agree,
## record, view, promote, repin and main.

(def sqlite ((import "std/sqlite")))
(def compress ((import "std/compress")))

# ── what main.lisp decides ───────────────────────────────────────────
# The run's settings, which main.lisp reads off the command line once every
# function in the module is defined, and assigns here. A function that reads
# one runs only after that, so each starts nil.
(def @host-program nil)
(def @test-timeout-ms nil)
(def @cas-dir nil)
(def @scratch-dir nil)
(def @run-build nil)
(def @ledgers nil)
(def @adopting-build nil)

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
# nothing else names a directory.
(defn state-dir []
  (or (env-dir "ELLE_STATE")
      (let [xdg (env-dir "XDG_STATE_HOME")]
        (if xdg (string xdg "/elle") nil))
      (let [home (env-dir "HOME")]
        (if home (string home "/.local/state/elle") nil)) (env-dir "ELLE_CACHE")
      "target"))

# ── schema (the subset of docs/test-store.md the runner creates) ─────
# ALTER a column into a table that predates it. On a fresh DB the CREATE
# already carries the column, so the ALTER fails with "duplicate column
# name" — that is the no-op path; protect swallows it.
(defn ensure-column [conn table col decl]
  (protect (sqlite:exec conn
                        (string "ALTER TABLE " table " ADD COLUMN " col " " decl)))
  nil)

# The code state a run row carries. Named once, because the CREATE and the
# migration below and the INSERT in main all have to agree about it.
(def run-code-columns
  [["git_commit" "TEXT"] ["git_dirty" "INTEGER"] ["tree_hash" "TEXT"]
   ["worktree" "TEXT"] ["boot_fingerprint" "INTEGER"] ["elle_version" "TEXT"]
   ["build_profile" "TEXT"] ["host" "TEXT"] ["argv" "TEXT"] ["run_key" "TEXT"]
   ["pid" "INTEGER"] ["build" "TEXT"]])

(defn ensure-columns [conn table cols]
  "ALTER each [col decl] of COLS into TABLE, as ensure-column does one."
  (each [col decl] in cols
    (ensure-column conn table col decl)))

(defn ensure-schema [conn]
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS run (id INTEGER PRIMARY KEY, started_at TEXT DEFAULT (datetime('now')), finished_at TEXT, run_key TEXT, tiers TEXT, selection TEXT, n_selected INTEGER, git_commit TEXT, git_dirty INTEGER, tree_hash TEXT, worktree TEXT, boot_fingerprint INTEGER, elle_version TEXT, build_profile TEXT, host TEXT, argv TEXT, pid INTEGER, build TEXT, n_pass INTEGER DEFAULT 0, n_fail INTEGER DEFAULT 0, n_skip INTEGER DEFAULT 0, n_diverge INTEGER DEFAULT 0, n_timeout INTEGER DEFAULT 0, wall_ms INTEGER, max_rss_kb INTEGER, cpu_user_ms INTEGER, cpu_sys_ms INTEGER)")
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS form (hash TEXT PRIMARY KEY, origin TEXT, session TEXT, file TEXT, form_index INTEGER, line INTEGER, col INTEGER, label TEXT, src TEXT, caps TEXT, touches TEXT, signal TEXT)")
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS result (id INTEGER PRIMARY KEY, run_id INTEGER, form_hash TEXT, tier TEXT, status TEXT, reason TEXT, expected TEXT, actual TEXT, syntax TEXT, signal TEXT, wall_ms INTEGER, cpu_us INTEGER, max_rss_kb INTEGER, jit_compiles INTEGER, jit_rejected INTEGER, jit_wall_us INTEGER, jit_cpu_us INTEGER, residue_regions INTEGER, compile_wall_us INTEGER, compile_cpu_us INTEGER)")
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS asset (result_id INTEGER, kind TEXT, hash TEXT, size INTEGER, codec TEXT)")
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS measurement (run_id INTEGER, result_id INTEGER, subject TEXT, axis TEXT, value REAL, half REAL, unit TEXT, bound REAL, kind TEXT, verdict TEXT)")
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS gauge (id INTEGER PRIMARY KEY, run_id INTEGER, file TEXT, heap TEXT, kind TEXT, delta INTEGER, reading INTEGER)")
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS changed_file (run_id INTEGER, path TEXT, status TEXT, blob_hash TEXT)")
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS compile_cost (run_id INTEGER, file TEXT, step TEXT, wall_us INTEGER, cpu_us INTEGER)")
  # Run honesty (docs/test-runner.md): finished_at is stamped
  # only when a run completes, so a NULL is the kill marker. Migrate session
  # DBs that predate the columns, then backfill the legacy rows, which have
  # n_selected IS NULL because new rows always set it. A legacy row that
  # reached its final counter aggregation had completed. One with no results
  # at all is assumed completed, since a zero-result selection is legal. Only
  # "results present but counters never aggregated" is evidence of a kill.
  # started_at is the best available stamp. New-schema rows are never
  # backfilled — for them the stamp is authoritative, however empty the run.
  (ensure-column conn "run" "finished_at" "TEXT")
  (ensure-column conn "run" "n_selected" "INTEGER")
  # A run recorded before the code-state columns existed keeps NULL for each
  # of them: the run happened, and nothing recorded what it ran against.
  (ensure-columns conn "run" run-code-columns)
  # A run recorded before its wall time and its process's cost were kept reads
  # NULL for each, as a run that never completed does: the summary prints no
  # time for either. So does a result recorded before a cost column existed.
  (ensure-columns conn "run"
                  [["wall_ms" "INTEGER"] ["max_rss_kb" "INTEGER"]
                   ["cpu_user_ms" "INTEGER"] ["cpu_sys_ms" "INTEGER"]])
  (ensure-columns conn "result"
                  [["max_rss_kb" "INTEGER"] ["jit_compiles" "INTEGER"]
                   ["jit_rejected" "INTEGER"] ["jit_wall_us" "INTEGER"]
                   ["jit_cpu_us" "INTEGER"] ["residue_regions" "INTEGER"]
                   ["compile_wall_us" "INTEGER"] ["compile_cpu_us" "INTEGER"]])
  # A gauge row recorded before the heap column existed was the runner's, and
  # reads NULL there (docs/test-gauges.md).
  (ensure-column conn "gauge" "heap" "TEXT")
  # A reading recorded before it carried its interval and the row it met keeps
  # NULL for each (docs/test-store.md).
  (ensure-column conn "measurement" "half" "REAL")
  (ensure-column conn "measurement" "bound" "REAL")
  (ensure-column conn "measurement" "kind" "TEXT")
  # What makes a run the same run in two stores, so an import of one artifact
  # lands it once (docs/test-store.md). SQLite holds every NULL
  # distinct under a unique index, so a run recorded before the key existed
  # keeps its row.
  (sqlite:exec conn
               "CREATE UNIQUE INDEX IF NOT EXISTS run_key_is_one_run ON run (run_key)")
  (sqlite:exec conn
               "UPDATE run SET finished_at = started_at WHERE finished_at IS NULL AND n_selected IS NULL AND (n_pass + n_fail + n_skip + n_diverge + n_timeout) > 0")
  (sqlite:exec conn
               "UPDATE run SET finished_at = started_at WHERE finished_at IS NULL AND n_selected IS NULL AND id NOT IN (SELECT DISTINCT run_id FROM result)"))

# ── the columns an import copies each table by ───────────────────────
# Beside the schema, so a column added to a table above lands here as well
# (import.lisp). One list per table, in the order the INSERT binds them, each
# starting with the column the import supplies: a run's key, or the run or
# result id the row is remapped onto. As data, no hand-written INSERT binds a
# value under the wrong name.
(def run-columns
  ["run_key" "started_at" "finished_at" "tiers" "selection" "n_selected"
   "git_commit" "git_dirty" "tree_hash" "worktree" "boot_fingerprint"
   "elle_version" "build_profile" "host" "argv" "pid" "build" "n_pass" "n_fail"
   "n_skip" "n_diverge" "n_timeout" "wall_ms" "max_rss_kb" "cpu_user_ms"
   "cpu_sys_ms"])

(def form-columns
  ["hash" "origin" "session" "file" "form_index" "line" "col" "label" "src"
   "caps" "touches" "signal"])

(def result-columns
  ["run_id" "form_hash" "tier" "status" "reason" "expected" "actual" "syntax"
   "signal" "wall_ms" "cpu_us" "max_rss_kb" "jit_compiles" "jit_rejected"
   "jit_wall_us" "jit_cpu_us" "residue_regions" "compile_wall_us"
   "compile_cpu_us"])

(def asset-columns ["result_id" "kind" "hash" "size" "codec"])

(def measurement-columns
  ["run_id" "result_id" "subject" "axis" "value" "half" "unit" "bound" "kind"
   "verdict"])

# The tables a run alone keys, as [table columns].
(def run-tables
  [["gauge" ["run_id" "file" "heap" "kind" "delta" "reading"]]
   ["changed_file" ["run_id" "path" "status" "blob_hash"]]
   ["compile_cost" ["run_id" "file" "step" "wall_us" "cpu_us"]]])

(defn last-rowid [conn]
  "The id the row just inserted landed at, which is how a run, a result and
   an imported row each learn the id their children have to name."
  (get (get (sqlite:query conn "SELECT last_insert_rowid() AS id") 0) :id))

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

# What the working tree differs from HEAD by. Non-empty output is the dirty
# flag, and the same listing is one of the three inputs to the tree hash.
(def status-cmd "git status --porcelain 2>/dev/null")

# The identity of the working tree, in one hash: the HEAD tree, the porcelain
# status, and the diff from HEAD. Two runs share it when they ran against the
# same code, dirty working tree included — which a commit alone cannot say.
# `git hash-object` does the hashing, so a large diff never crosses into the
# runner.
(def tree-hash-cmd
  (string "{ git rev-parse 'HEAD^{tree}'; " status-cmd "; "
          "git diff HEAD --binary; } 2>/dev/null | git hash-object --stdin 2>/dev/null"))

# What names this run in any store that holds it (docs/test-store.md). The
# machine, the process and the instant are what separate two runs
# that share every other column — two shards of one CI matrix, on one image,
# on one commit, with one argv.
(defn run-key [host argv]
  (string (hash (string host "|" (sys/pid) "|" (clock/realtime) "|"
                        (clock/monotonic) "|" argv))))

# The code state and the machine this run ran on. Outside a repository the git
# fields are nil, which lands as SQL NULL: the run happened, and nothing names
# the code it ran against. The host is a fact about the box, so it is recorded
# either way, and so is the pid. With the host, the pid tells a run still in
# flight from a killed one (view.lisp). The build is the runner's own, and nil
# outside the rig (main.lisp).
#
# The boot fingerprint is the binary itself, hashed (docs/test-store.md): a
# commit says which sources a run was meant to test, and only this says which
# executable tested them.
(defn run-identity []
  (let [commit (capture-cmd "git rev-parse HEAD 2>/dev/null")
        host (capture-cmd "uname -n")
        argv (string/join (rest (sys/argv)) " ")]
    (struct :commit commit
            :dirty (if commit (if (capture-cmd status-cmd) 1 0) nil)
            :tree (if commit (capture-cmd tree-hash-cmd) nil)
            :worktree (capture-cmd "git rev-parse --show-toplevel 2>/dev/null")
            :boot (elle/boot-fingerprint) :host host :version (elle/version)
            :profile (elle/build-profile) :argv argv :key (run-key host argv)
            :pid (sys/pid) :build run-build)))

# ── CAS: content-addressed artifact store (docs/test-runner.md) ─────
# Artifacts (the --dump bodies) live on disk under `cas-dir` (a sibling of the
# session DB), content-addressed and zstd-compressed; the DB stores only the
# hash/size/codec. Identical artifacts across forms, tiers, and runs dedup to
# one file. `cas-addr` reuses the SAME builtin hash the runner uses for form
# identity (64-bit, build-stable — all a disposable local cache needs;
# docs/test-runner.md holds the cross-machine upgrade path).
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

# --dump capture is off (docs/test-runner.md): no
# compile/dumps call, no dump asset rows, no CAS dump files. stdout/stderr
# capture is a separate path, capture-stdio, on the per-form execution.
(defn capture-dumps [src name]
  [])

# Insert one asset row (dump = [kind addr size codec]) for a result.
(defn insert-asset [conn result-id dump]
  (sqlite:exec conn
               "INSERT INTO asset (result_id, kind, hash, size, codec) VALUES (?1,?2,?3,?4,?5)"
               [result-id (get dump 0) (get dump 1) (get dump 2) (get dump 3)]))

(defn insert-assets [conn result-id dumps]
  (each d in dumps
    (insert-asset conn result-id d)))

# CAS-store a result's captured stdout/stderr (only when non-empty, so a silent
# test adds no asset rows) and record them as `stdout`/`stderr` assets.
(defn capture-stdio [conn result-id out-str err-str]
  (when (and out-str (> (length out-str) 0))
    (insert-asset conn result-id (concat ["stdout"] (cas-put out-str))))
  (when (and err-str (> (length err-str) 0))
    (insert-asset conn result-id (concat ["stderr"] (cas-put err-str)))))
