(elle/epoch 14)
# audited: 2026-10-05
## elle test — the session store: where a run is kept, the schema it is kept
## in, what a run row says about the code it ran against, and the CAS.
## docs/test-store.md
##
## A fragment of one module: src/program/subcommand.rs concatenates the
## src/test files in order and compiles the result as the `elle test`
## subcommand, so a run needs no source tree. Bindings resolve across the
## whole concatenation.

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

# ── schema (the subset of docs/test-store.md the runner creates) ─────
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
   ["worktree" "TEXT"] ["boot_fingerprint" "INTEGER"] ["elle_version" "TEXT"]
   ["build_profile" "TEXT"] ["host" "TEXT"] ["argv" "TEXT"] ["run_key" "TEXT"]
   ["pid" "INTEGER"] ["build" "TEXT"]])

(defn ensure-code-columns [conn cols]
  (if (empty? cols)
    nil
    (begin
      (ensure-column conn "run" (get (first cols) 0) (get (first cols) 1))
      (ensure-code-columns conn (rest cols)))))

(defn ensure-schema [conn]
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS run (id INTEGER PRIMARY KEY, started_at TEXT DEFAULT (datetime('now')), finished_at TEXT, run_key TEXT, tiers TEXT, selection TEXT, n_selected INTEGER, git_commit TEXT, git_dirty INTEGER, tree_hash TEXT, worktree TEXT, boot_fingerprint INTEGER, elle_version TEXT, build_profile TEXT, host TEXT, argv TEXT, pid INTEGER, build TEXT, n_pass INTEGER DEFAULT 0, n_fail INTEGER DEFAULT 0, n_skip INTEGER DEFAULT 0, n_diverge INTEGER DEFAULT 0, n_timeout INTEGER DEFAULT 0)")
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS form (hash TEXT PRIMARY KEY, origin TEXT, session TEXT, file TEXT, form_index INTEGER, line INTEGER, col INTEGER, label TEXT, src TEXT, caps TEXT, touches TEXT, signal TEXT)")
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS result (id INTEGER PRIMARY KEY, run_id INTEGER, form_hash TEXT, tier TEXT, status TEXT, reason TEXT, expected TEXT, actual TEXT, syntax TEXT, signal TEXT, wall_ms INTEGER, cpu_us INTEGER, max_rss_kb INTEGER)")
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS asset (result_id INTEGER, kind TEXT, hash TEXT, size INTEGER, codec TEXT)")
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS measurement (run_id INTEGER, result_id INTEGER, subject TEXT, axis TEXT, value REAL, half REAL, unit TEXT, bound REAL, kind TEXT, verdict TEXT)")
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS gauge (id INTEGER PRIMARY KEY, run_id INTEGER, file TEXT, heap TEXT, kind TEXT, delta INTEGER, reading INTEGER)")
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
  # A result recorded before a child's peak was kept reads NULL for it.
  (ensure-column conn "result" "max_rss_kb" "INTEGER")
  # A gauge row recorded before the heap column existed was the runner's, and
  # reads NULL there (docs/test-gauges.md).
  (ensure-column conn "gauge" "heap" "TEXT")
  # A reading recorded before it carried its interval and the row it met keeps
  # NULL for each (docs/test-store.md § Measurements).
  (ensure-column conn "measurement" "half" "REAL")
  (ensure-column conn "measurement" "bound" "REAL")
  (ensure-column conn "measurement" "kind" "TEXT")
  # What makes a run the same run in two stores, so an import of one artifact
  # lands it once (docs/test-store.md § The run key). SQLite holds every NULL
  # distinct under a unique index, so a run recorded before the key existed
  # keeps its row.
  (sqlite:exec conn
               "CREATE UNIQUE INDEX IF NOT EXISTS run_key_is_one_run ON run (run_key)")
  (sqlite:exec conn
               "UPDATE run SET finished_at = started_at WHERE finished_at IS NULL AND n_selected IS NULL AND (n_pass + n_fail + n_skip + n_diverge + n_timeout) > 0")
  (sqlite:exec conn
               "UPDATE run SET finished_at = started_at WHERE finished_at IS NULL AND n_selected IS NULL AND id NOT IN (SELECT DISTINCT run_id FROM result)"))

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

# The code state and the machine this run ran on. Outside a repository the git
# fields are nil, which lands as SQL NULL: the run happened, and nothing names
# the code it ran against. The host is a fact about the box, so it is recorded
# either way, and so is the pid: with the host, it is what tells a run still in
# flight from a killed one (view.lisp). The build is the runner's own, and nil
# outside the rig (main.lisp).
#
# The boot fingerprint is the binary itself, hashed (docs/test-store.md § The
# boot fingerprint): a commit says which sources a run was meant to test, and
# only this says which executable tested them.
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

# What names this run in any store that holds it (docs/test-store.md § The run
# key). The machine, the process and the instant are what separate two runs
# that share every other column — two shards of one CI matrix, on one image,
# on one commit, with one argv.
(defn run-key [host argv]
  (string (hash (string host "|" (sys/pid) "|" (clock/realtime) "|"
                        (clock/monotonic) "|" argv))))

# ── CAS: content-addressed artifact store (docs/test-runner.md § CAS) ─────
# Artifacts (the --dump bodies) live on disk under `cas-dir` (a sibling of the
# session DB), content-addressed and zstd-compressed; the DB stores only the
# hash/size/codec. Identical artifacts across forms, tiers, and runs dedup to
# one file. `cas-addr` reuses the SAME builtin hash the runner uses for form
# identity (64-bit, build-stable — all a disposable local cache needs;
# docs/test-runner.md § CAS asset capture holds the cross-machine upgrade path).
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

# --dump capture is off (docs/test-runner.md § CAS asset capture): no
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
  (if (empty? dumps)
    nil
    (begin
      (insert-asset conn result-id (first dumps))
      (insert-assets conn result-id (rest dumps)))))

# ── the heap gauges (docs/test-gauges.md) ──────────────────────────
# Every gauge the runner reads, as [kind reader]. Each primitive is Immediate,
# so a call allocates nothing and cannot move the number it reports. One list,
# because the runner's sampling, the workers' readings and the growers query in
# the views all have to agree about which gauges there are.
#
# The runner reads them on two heaps. Its own heap sees what it does per file:
# the compile, the syntax it holds, and the rows it writes. A worker thread has
# its own VM and heap, so each worker reads the same list around its form's
# tiered call and hands back the readings (gauges-around).
(def heap-gauges
  [["objects" (fn [] (arena/count))] ["regions" (fn [] (arena/region-count))]
   ["pages" (fn [] (arena/page-claims))]
   ["region-frees" (fn [] (arena/region-frees))]
   ["page-frees" (fn [] (arena/page-frees))]
   ["object-frees" (fn [] (arena/object-frees))]
   ["one-page-frees" (fn [] (arena/one-page-frees))]
   ["empty-frees" (fn [] (arena/empty-frees))]
   ["one-object-frees" (fn [] (arena/one-object-frees))]
   ["few-object-frees" (fn [] (arena/few-object-frees))]
   ["many-object-frees" (fn [] (arena/many-object-frees))]
   ["adopts" (fn [] (arena/adopts))]
   ["adopts-into-empty" (fn [] (arena/adopts-into-empty))]
   ["owned" (fn [] (arena/owned))] ["owned-frees" (fn [] (arena/owned-frees))]
   ["owned-free-pages" (fn [] (arena/owned-free-pages))]
   ["owned-free-objects" (fn [] (arena/owned-free-objects))]
   ["owned-one-page-frees" (fn [] (arena/owned-one-page-frees))]
   ["rescues" (fn [] (arena/rescues))]
   ["rescue-survivors" (fn [] (arena/rescue-survivors))]
   ["extracts" (fn [] (arena/extracts))] ["reparents" (fn [] (arena/reparents))]])

# The gauge names alone, in the order a reading and a rendering both take them.
(defn gauge-kinds []
  (map (fn [g] (get g 0)) heap-gauges))

# The reading every window starts from, as kind → value. The runner keeps one
# of these and replaces it at each boundary.
(defn gauge-baseline []
  (let [@prev @{}]
    (each g in heap-gauges
      (put prev (get g 0) ((get g 1))))
    prev))

# ── a worker's readings: the test heap ───────────────────────────────
# Every reading lands in slots allocated before the first, one per gauge. The
# reading loop still costs something, and the same every time, so a reading
# taken right after another measures that cost (gauge-charge).
(defn gauge-slots []
  (let [@slots @[]]
    (each g in heap-gauges
      (push slots 0))
    slots))

(defn read-gauges [slots]
  (var i 0)
  (each g in heap-gauges
    (put slots i ((get g 1)))
    (assign i (+ i 1))))

(defn gauges-around [thunk]
  "Call THUNK and answer [its-value readings]. READINGS are three readings of
   every gauge: two back to back before the call, then one after it. The
   worker computes nothing after the call; the runner does (gauge-charge)."
  (let [warm (gauge-slots)
        before (gauge-slots)
        after (gauge-slots)]
    (read-gauges warm)
    (read-gauges before)
    (let [v (thunk)]
      (read-gauges after)
      [v [warm before after]])))

(defn gauge-charge [readings i]
  "What the call cost gauge I: its change across the call, less the change a
   reading alone makes."
  (let [[warm before after] readings]
    (- (- (get after i) (get before i)) (- (get before i) (get warm i)))))

# The test heap's sums for the file in hand, one per gauge in list order, or
# nil while no run of the file has handed any back. A file whose runs all came
# back empty records no test rows, which says it was not measured.
(def @test-gauge-sums nil)

(defn add-test-gauges [readings]
  "Add one run's charges to the file's sums."
  (when (= test-gauge-sums nil) (assign test-gauge-sums (gauge-slots)))
  (var i 0)
  (each g in heap-gauges
    (put test-gauge-sums i (+ (get test-gauge-sums i) (gauge-charge readings i)))
    (assign i (+ i 1))))

(defn test-gauge-mark [conn run-id file]
  "Write the file's test-heap sums, one row per gauge, and start the next file
   empty. A worker heap lives for one run, so a row has no reading to chain."
  (when test-gauge-sums
    (var i 0)
    (each kind in (gauge-kinds)
      (sqlite:exec conn
                   "INSERT INTO gauge (run_id, file, heap, kind, delta, reading) VALUES (?1,?2,'test',?3,?4,NULL)"
                   [run-id (string file) kind (get test-gauge-sums i)])
      (assign i (+ i 1)))
    (assign test-gauge-sums nil))
  nil)

# One boundary: charge each gauge's change since the previous boundary to FILE,
# and leave this reading as the next window's start.
#
# ONE reading per boundary, not a before/after pair around each file. A pair
# leaves the rows written between them charged to nobody, and that gap is where
# the runner's own work lives. With one reading every object the runner
# allocates lands in exactly one file's window, and the readings chain: a
# file's `reading` plus the next file's `delta` is the next file's `reading`.
(defn gauge-mark [conn run-id prev file]
  (each g in heap-gauges
    (let [kind (get g 0)
          now ((get g 1))]
      (sqlite:exec conn
                   "INSERT INTO gauge (run_id, file, heap, kind, delta, reading) VALUES (?1,?2,'runner',?3,?4,?5)"
                   [run-id (string file) kind (- now (get prev kind)) now])
      (put prev kind now)))
  nil)

# CAS-store a result's captured stdout/stderr (only when non-empty, so a silent
# test adds no asset rows) and record them as `stdout`/`stderr` assets.
(defn capture-stdio [conn result-id out-str err-str]
  (if (and (not (= out-str nil)) (> (length out-str) 0))
    (insert-asset conn result-id (concat ["stdout"] (cas-put out-str)))
    nil)
  (if (and (not (= err-str nil)) (> (length err-str) 0))
    (insert-asset conn result-id (concat ["stderr"] (cas-put err-str)))
    nil))
