(elle/epoch 12)
## elle test — the agent-first test runner. See docs/test-runner.md.
##
## Execution core (per-form fault-barrier compilation mode, docs § Mechanism):
##   compile the whole FILE once via (compile/barrier-module SRC NAME) — the real
##   file-compilation path (epoch + whole-module analysis: shared bindings,
##   forward references, capture/signal inference). It runs the file's def/var
##   forms eagerly to establish the shared environment and hands back one 0-arg
##   THUNK per test form, each capturing that environment. We then run each thunk
##   on each tier via (compile/run-on TIER thunk) under (protect ...) in a worker.
##   This preserves the *typed* failure signal (e.g. {:error :failed-assertion
##   ...}) — no eval stringification, no subprocess, no stderr scraping.
##
## CAS asset capture (docs/test-runner.md § CAS asset capture): each file is
## compiled once more via (compile/dumps SRC NAME) to obtain its --dump artifacts
## (ast/fhir/.../lir) as strings; each non-empty artifact is content-addressed
## and zstd-compressed into <dir-of-db>/cas/<hash> (deduped), and one `asset` row
## per (form × tier) result references it by hash. So an agent reads the LIR of a
## failing form from the CAS by hash instead of re-running `elle --dump=lir`.
##
## stdout/stderr capture: each tier run executes under a worker-side (ev/run ...)
## (it rides into the bundle now that parameters are sendable), with *stdout*/
## *stderr* rebound to temp files; non-empty output becomes `stdout`/`stderr`
## assets per (form × tier). See exec-thunk-capture.
##
## Per-test timeout: --timeout MS (default 60000) bounds each form's worker via
## os/join's deadline; an over-budget form is recorded `timeout` and gates
## non-zero. `--trace=KW` is split off by the `test` subcommand and applied to
## the runner's VM/free-log (e.g. `--trace=free` to attribute a UAF); the runner
## itself does not interpret it. NOT YET: --changed/--rerun-failed.

(def sqlite ((import "std/sqlite")))
(def compress ((import "std/compress")))

# ── argv ─────────────────────────────────────────────────────────────
# (rest (sys/argv)) drops the program name; tolerate a leading "--" so the
# runner works both as `elle test ...` and standalone as `elle src/test.lisp -- ...`.
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

# ── schema (subset of docs/test-runner.md the v1 runner populates) ───
# ALTER a column into a table that predates it. On a fresh DB the CREATE
# already carries the column, so the ALTER fails with "duplicate column
# name" — that is the no-op path; protect swallows it.
(defn ensure-column [conn table col decl]
  (protect (sqlite:exec conn
                        (string "ALTER TABLE " table " ADD COLUMN " col " " decl)))
  nil)

(defn ensure-schema [conn]
  (sqlite:exec conn
               "CREATE TABLE IF NOT EXISTS run (id INTEGER PRIMARY KEY, started_at TEXT DEFAULT (datetime('now')), finished_at TEXT, tiers TEXT, selection TEXT, n_selected INTEGER, n_pass INTEGER DEFAULT 0, n_fail INTEGER DEFAULT 0, n_skip INTEGER DEFAULT 0, n_diverge INTEGER DEFAULT 0, n_timeout INTEGER DEFAULT 0)")
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
  (sqlite:exec conn
               "UPDATE run SET finished_at = started_at WHERE finished_at IS NULL AND n_selected IS NULL AND (n_pass + n_fail + n_skip + n_diverge + n_timeout) > 0")
  (sqlite:exec conn
               "UPDATE run SET finished_at = started_at WHERE finished_at IS NULL AND n_selected IS NULL AND id NOT IN (SELECT DISTINCT run_id FROM result)"))

# ── label: scavenge the first assert message from a form's syntax ────
(defn scan-msg [x]
  (if (list? x)
    (if (and (>= (length x) 3) (= (first x) (quote assert)) (string? (get x 2)))
      (get x 2)
      (scan-children x))
    nil))
(defn scan-children [xs]
  (if (empty? xs)
    nil
    (let [m (scan-msg (first xs))]
      (if m m (scan-children (rest xs))))))

# ── execution core ───────────────────────────────────────────────────
# Test code is untrusted: it can corrupt VM state, loop, or exhaust resources.
# The whole FILE is compiled ONCE, in the fault-barrier test mode, by
# `compile/barrier-module` (docs/test-runner.md § Mechanism): def/var forms run
# eagerly to establish the file's shared bindings, and each test (expression)
# form is returned as a 0-arg THUNK closure capturing that shared environment.
# We then run each thunk on each tier here.
#
# `exec-thunk` runs one such thunk on a tier, fault-isolated: spawn a worker
# (own VM), force the closure onto the tier under `protect`, and `os/join`
# marshals back the structured [ok? payload]. The barrier lives OUTSIDE the
# tiered closure deliberately — a fiber-based catch INSIDE a closure handed to
# compile/run-on is rejected by the optimizing tiers (they cannot create the
# handler closure), so per-form catching is a bytecode-tier property and the
# optimizing tiers run only the forms that complete normally. compile/run-on
# preserves the typed failure signal (e.g. {:error :failed-assertion ...}).
# This probe path joins with no deadline (its closures are trivial and always
# finish); real test forms run under exec-thunk-capture, which bounds the join
# with `test-timeout-ms` so a hung test is recorded `timeout`, not a wedge.
(defn exec-thunk [tier thunk]
  (os/join (os/spawn-vm (fn [] (protect (compile/run-on tier thunk))))))

# Worker tier for the per-form execution (exec-thunk-capture). Default is the
# LIGHT worker (sys/spawn-vm, primitives only, ~1.6ms): a sliced single-form
# thunk captures its already-imported deps from the eager main-VM setup, so it
# needs no stdlib in the worker. A whole-file thunk (legacy multi-form mode) runs
# the file's OWN `import`/`eval` inside the worker, which needs the heavy worker
# (sys/spawn, full stdlib ~ the main VM) — process-whole rebinds this to true.
(def *heavy-worker* (make-parameter false))
(defn worker-spawn [closure]
  (if (*heavy-worker*) (os/spawn closure) (os/spawn-vm closure)))

# Like exec-thunk, but also CAPTURE the test's stdout/stderr.
#
# A spawned worker has only primitives — no scheduler — so any async I/O the test
# does (println, port/open, sockets) would yield into the void. We give the worker
# a real runtime the cheap way: the closure we ship references `ev/run` (stdlib),
# so the serializer drags `ev/run`'s whole closure graph into the bundle (this is
# exactly what made parameters sendable buy us — *stdout*/*stderr* and everything
# ev/run closes over now cross the boundary). The worker runs the tiered call
# under that ev/run, with `*stdout*`/`*stderr*` rebound to temp files; it slurps
# and deletes them and marshals [result stdout stderr] back through os/join.
#
# Returns a struct {:result [ok? payload] :stdout S :stderr S}. The whole spawn is
# wrapped in `protect` so a serialization failure (a test capturing an unsendable
# value) is recorded as a fail result instead of crashing the run.
# Run the tiered call with *stdout*/*stderr* rebound to temp files, returning
# {:result [ok? payload] :stdout S :stderr S}. Assumes a scheduler is running
# (port I/O yields): the worker supplies its own via ev/run; the in-process
# fallback relies on the runner's top-level ev/run.
(defn capture-run [tier thunk out-path err-path]
  (let [op (port/open out-path :write)
        ep (port/open err-path :write)]
    (sys/trap-exit! true)
    (let [v (parameterize ((*stdout* op)
                           (*stderr* ep))
              (protect (compile/run-on tier thunk)))]
      (sys/trap-exit! false)
      (port/close op)
      (port/close ep)
      (let [so (slurp out-path)
            se (slurp err-path)]
        (file/delete out-path)
        (file/delete err-path)
        (struct :result v :stdout so :stderr se)))))

# A spawn that can't deep-copy the test thunk — because it captures an
# unsendable value (FFI handle, compile/* artifact, fiber, file/socket port) —
# raises :thread-error whose message mentions sending/serializing. Distinguished
# from a worker panic so ONLY truly-unhostable forms take the unisolated
# in-process path (a panic stays a fail rather than crashing the main VM).
(defn serialization-error? [payload]
  (and (= (get payload :error) :thread-error)
       (let [m (string (get payload :message))]
         (or (string/contains? m "send") (string/contains? m "serialize")))))

(defn exec-thunk-capture [tier thunk out-path err-path]
  (let [outcome (protect (os/join (worker-spawn (fn []
                                    (ev/run (fn []
                                      (capture-run tier thunk out-path err-path)))))
                                  test-timeout-ms))]
    (if (get outcome 0)
      (get outcome 1)  # The worker spawn/join failed. If the thunk simply can't cross into a
      # worker (unsendable capture), run it IN-PROCESS — no isolation, no
      # timeout, but it runs (docs/test-runner.md § Isolation). Any other
      # thread-error (e.g. a worker panic) stays a recorded fail.
      (if (serialization-error? (get outcome 1))
        # In-process runs share the MAIN VM, so a form that sets :trace (e.g.
        # config.lisp / trace.lisp toggling :call) and never clears it — an
        # assert aborts first — would leave the runner's own machinery traced.
        # Save and restore the main VM's trace around the run to contain it
        # (worker runs are already isolated by their fresh VM).
        (let [saved-trace (vm/config :trace)
              r (capture-run tier thunk out-path err-path)]
          (vm/config-set :trace saved-trace)
          r)
        (struct :result [false (get outcome 1)] :stdout "" :stderr "")))))

# Run THUNK as a scheduled, PUMPED fiber and capture its stdout/stderr. Elle has
# NO synchronous I/O — every port/socket/subprocess op yields an io-request — so
# the file's TOP-LEVEL I/O is only serviced if the thunk runs as a fiber under a
# running scheduler. `(spawn thunk)` adds it to the harness's `evrun` scheduler
# and `(join …)` pumps it to completion; running it inline via `compile/run-on`
# never schedules it, so a whole-file script doing its own I/O (not just inside
# ev/spawn'd sub-fibers) would escape an io-request. The thunk shares the ONE
# `evrun` scheduler (no nested ev/run — that crashes files like process.lisp that
# start their own scheduler). EVRUN/SPAWN/JOIN/OUT/ERR are passed so the caller
# supplies the SAME stdlib instance the thunk uses (the worker's, or the main
# VM's for the in-process fallback). Returns {:result [ok? value] :stdout :stderr}.
(defn
  capture-pumped
  [evrun spawn join out-param err-param thunk out-path err-path]
  (evrun (fn []
           (let [op (port/open out-path :write)
                 ep (port/open err-path :write)]
             (sys/trap-exit! true)
             (let [v (parameterize ((out-param op)
                                    (err-param ep))
                       (protect (join (spawn thunk))))]
               (sys/trap-exit! false)
               (port/close op)
               (port/close ep)
               (let [so (slurp out-path)
                     se (slurp err-path)]
                 (file/delete out-path)
                 (file/delete err-path)
                 (struct :result v :stdout so :stderr se)))))))

# Whole-file (legacy multi-form) execution. Unlike exec-thunk-capture — which
# ships a MAIN-compiled thunk — this ships the file's parsed SYNTAX (sendable via
# os/spawn) and the worker compiles it with compile/whole-module-syntax against
# its OWN stdlib, then runs it (pumped) under the WORKER's ev/run. That is what
# makes the file's runtime `import`s and the scheduler agree on dynamic params: a
# main-compiled thunk binds `ev/spawn`/`*spawn*` to the MAIN stdlib, but a module
# the thunk `import`s at runtime resolves to the WORKER stdlib — two distinct
# *spawn* parameter objects, so any file whose forms import a module that yields
# (sync/redis/http2/process/grpc) breaks. ev/run, *stdout*, *stderr* are resolved
# IN the worker (eval) for the same reason — they must be the worker's parameter
# objects. Syntax compiles in the heavy worker (it runs the file's own
# import/eval), so os/spawn (not -vm). `policy` is the JIT policy (:off / :eager,
# see whole-file-policies): the worker sets it via (vm/config-set :jit policy)
# before running, so the SAME file runs under bytecode and under JIT — the
# smoke-vm/smoke-jit split. The worker's VM is fresh, so the policy is isolated;
# the in-process fallback saves and restores the main VM's policy around the run.
# (vm/config-set, not (put (vm/config) …) — the put→set analyzer rewrite the docs
# describe does not fire here; the direct setter is what actually mutates the VM.)
(defn exec-source-capture [policy forms name out-path err-path]
  (let [outcome (protect (os/join (os/spawn (fn []
                                    (vm/config-set :jit policy)
                                    (let [w-evrun (eval (quote ev/run))
                                      w-spawn (eval (quote ev/spawn))
                                      w-join (eval (quote ev/join))
                                      w-out (eval (quote *stdout*))
                                      w-err (eval (quote *stderr*))
                                      thunk (get (get (compile/whole-module-syntax forms
                                      name) 0) 1)]
                                      (capture-pumped w-evrun w-spawn w-join
                                      w-out w-err thunk out-path err-path))))
                                  test-timeout-ms))]
    (if (get outcome 0)
      (get outcome 1)  # Unsendable RESULT (an orphan fiber, an io-request, …) can't cross back
      # through os/join. Fall back to running IN-PROCESS — no isolation, no
      # timeout — compiling the same syntax against the MAIN stdlib and running
      # under the runner's own ev/run + *stdout*/*stderr* (all main-consistent),
      # exactly as exec-thunk-capture does for unsendable captures. The main VM's
      # JIT policy is set for the run and restored after (it is shared, not fresh).
      (if (serialization-error? (get outcome 1))
        (let [saved (vm/config :jit)
              saved-trace (vm/config :trace)
              thunk (get (get (compile/whole-module-syntax forms name) 0) 1)]
          (vm/config-set :jit policy)
          (let [r (capture-pumped ev/run ev/spawn ev/join *stdout* *stderr*
                                  thunk out-path err-path)]
            (vm/config-set :jit saved)
            # Restore the main VM's trace too: a whole-file form that sets
            # :trace and aborts before clearing must not bleed into the runner.
            (vm/config-set :trace saved-trace)
            r))
        (struct :result [false (get outcome 1)] :stdout "" :stderr "")))))

(defn sig-of [payload]
  (let [e (get payload :error)]
    (if e (string ":" (string e)) ":error")))
# render keyword with its leading colon

# Render a payload field to text, or nil when absent (so it lands as SQL NULL).
(defn field-str [payload key]
  (let [v (get payload key)]
    (if (= v nil) nil (string v))))

# ── CAS: content-addressed artifact store (docs § CAS asset capture) ─────
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

# ── tier set: probe which backends this build carries ────────────────
# compile/run-on answers :tier-rejected/:feature-disabled for a tier whose
# feature wasn't compiled in. Such a tier is dropped from the run entirely (a
# feature the binary lacks is not a coverage gap of THIS build). A tier that is
# present but can't run a particular form answers :ineligible — that is a
# per-form skip (see classify), not an absent tier.
(defn feature-disabled? [r]
  (and (not (get r 0)) (= (get (get r 1) :error) :tier-rejected)
       (= (get (get r 1) :reason) :feature-disabled)))

(defn tier-available? [tk]
  (not (feature-disabled? (exec-thunk tk (fn [] 0)))))
# probe with a trivial closure

# Candidate tiers as [tier-keyword tier-label]; :bytecode is recorded as :vm.
# The label is a keyword (result.tier is keyword-typed; sqlite stores its name).
(def candidate-tiers
  [[:bytecode :vm] [:jit :jit] [:wasm :wasm] [:mlir-cpu :mlir-cpu]])

(def active-tiers (filter (fn [p] (tier-available? (get p 0))) candidate-tiers))

# A whole-file (legacy multi-form) thunk is a yielding imperative script: it runs
# under the worker's full scheduler, NOT forced onto a backend via compile/run-on
# (that only fits a single non-yielding form). So instead of a tier we vary the
# JIT POLICY it runs under — :off (pure bytecode, recorded "vm") and :eager (JIT
# every function, recorded "jit") — set per-worker via (put (vm/config) :jit …).
# That is exactly the old smoke-vm + smoke-jit split, folded into one run. :eager
# is included only when this build carries the JIT. Each entry is [policy label].
# No value-divergence is judged across policies (process-whole passes diverge?
# false): a script's pids/timestamps differ run-to-run by design.
(def whole-file-policies
  (concat [[:off :vm]] (if (tier-available? :jit) [[:eager :jit]] [])))

# Comma-joined tier labels for the run.tiers column. Labels are keywords, so
# stringify each (`(string :vm)` → "vm").
(defn tiers-str [tiers]
  (if (empty? tiers)
    ""
    (if (empty? (rest tiers))
      (string (get (first tiers) 1))
      (string (string (get (first tiers) 1)) "," (tiers-str (rest tiers))))))

# ── classify one tier's [ok? payload] into a row ─────────────────────
# pass  — the closure returned a value (held in :value for divergence checking).
# skip  — a loud gate fired (:gated) or the tier rejected this form (:ineligible).
# fail  — any other error; capture the assert payload (:syntax/:actual/:expected).
(defn classify [r]
  (let [ok (get r 0)
        raw (get r 1)  # A failure payload is normally a typed-error struct, but a worker can
        # hand back a bare unsendable object (an io-request, a fiber) as the
        # "error" — coerce anything non-struct so classify never faults the run.
        payload (if (struct? raw)
                  raw
                  (struct :error :error :message (string raw)))]
    (if ok
      (struct :status :pass :ok true :value raw)
      (let [err (get payload :error)]
        (if (= err :gated)
          (struct :status :skip :ok false :reason (field-str payload :reason))  # A trapped (exit): exit 0 is an opt-out (skip), any other code a fail.
          # See sys/trap-exit! — the runner traps exit so a test can't truncate
          # the run; this records what the test asked for.
          (if (= err :exited)
            (if (= (get payload :code) 0)
              (struct :status :skip :ok false :reason "exit 0")
              (struct :status :fail :ok false :sig ":exited"
                      :reason (field-str payload :message)))
            (if (= err :timeout)
              (struct :status :timeout :ok false
                      :reason (field-str payload :message))
              (if (and (= err :tier-rejected)
                       (= (get payload :reason) :ineligible))
                (struct :status :skip :ok false
                        :reason (field-str payload :message))
                (struct :status :fail :ok false :sig (sig-of payload)
                        :reason (field-str payload :message)
                        :syn (field-str payload :syntax)
                        :act (field-str payload :actual)
                        :exp (field-str payload :expected))))))))))

# Insert one (form × tier) result row and return its rowid (so assets can
# reference it).
(defn insert-result [conn run-id h tier-str c]
  (sqlite:exec conn
               "INSERT INTO result (run_id, form_hash, tier, status, reason, signal, syntax, expected, actual) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9)"
               [run-id h tier-str (get c :status) (get c :reason) (get c :sig)
                (get c :syn) (get c :exp) (get c :act)])
  (get (get (sqlite:query conn "SELECT last_insert_rowid() AS id") 0) :id))

# Run a form on every active tier, inserting one row per tier and attaching the
# file's captured `dumps` (a list of [kind addr size codec]) as assets to each.
# `exec-fn` is (fn [tier-keyword out-path err-path] -> {:result :stdout :stderr});
# the per-form path closes over a MAIN-compiled thunk (exec-thunk-capture), the
# whole-file path closes over the file's syntax (exec-source-capture). Returns
# [statuses pass-pairs]: the per-tier status strings, and [[tier-str value]...]
# for the tiers that returned a value (divergence candidates).
(defn run-tiers [conn run-id h exec-fn tiers dumps statuses pass-pairs]
  (if (empty? tiers)
    [statuses pass-pairs]
    (let [tp (first tiers)
          tk (get tp 0)
          ts (get tp 1)
          base (string scratch-dir "/" run-id "_" h "_" ts)
          cap (exec-fn tk (string base ".out") (string base ".err"))
          c (classify (get cap :result))
          rid (insert-result conn run-id h ts c)]
      (insert-assets conn rid dumps)
      (capture-stdio conn rid (get cap :stdout) (get cap :stderr))
      (run-tiers conn run-id h exec-fn (rest tiers) dumps
                 (concat statuses [(get c :status)])
                 (if (get c :ok)
                   (concat pass-pairs [[ts (get c :value)]])
                   pass-pairs)))))

# Are all of these values equal to the first? (Divergence = NOT all equal.)
(defn all-equal? [x xs]
  (if (empty? xs)
    true
    (and (= x (first xs)) (all-equal? x (rest xs)))))

# Render the passing tiers' values for the synthetic diverge row's reason.
(defn render-pairs [pairs]
  (if (empty? pairs)
    ""
    (let [p (first pairs)
          one (string (get p 0) "=" (string (get p 1)))]
      (if (empty? (rest pairs))
        one
        (string one " " (render-pairs (rest pairs)))))))

# Record one form across all active tiers; return the list of row-statuses
# (one per tier, plus "diverge" when the tiers that produced values disagree).
# Divergence is judged only over tiers that returned a value (status=pass);
# distinct values among them append a single synthetic tier='*' diverge row.
# Record ONE test form: insert its `form` row (metadata derived from the
# unevaluated source form `form`) and run its `thunk` across every active tier.
# Record ONE thunk under the form identity (SRC, LABEL) at index IDX: insert its
# `form` row and run it across every active tier (divergence appended as a
# synthetic tier='*' row). Shared by the per-form path (record-form-result) and
# the whole-file path (the legacy multi-form mode runs one thunk for the file).
# `diverge?` enables the synthetic tier='*' divergence row. The per-form path
# (record-form-result) forces ONE form onto each backend, so distinct values are
# a real cross-tier disagreement — diverge? true. The whole-file path runs an
# imperative SCRIPT under each JIT policy, whose side effects (pids, timestamps)
# differ run-to-run, so a value difference is NOT a bug — diverge? false.
(defn
  record-thunk
  [conn run-id origin file idx src label exec-fn dumps tiers diverge?]
  (let [h (string (hash src))]
    (sqlite:exec conn
                 "INSERT OR IGNORE INTO form (hash, origin, file, form_index, label, src) VALUES (?1,?2,?3,?4,?5,?6)"
                 [h (string origin) (string file) idx label src])
    (let [tr (run-tiers conn run-id h exec-fn tiers dumps [] [])
          statuses (get tr 0)
          pass-pairs (get tr 1)
          vals (map (fn [pp] (get pp 1)) pass-pairs)]
      (if (and diverge? (> (length pass-pairs) 1)
               (not (all-equal? (first vals) (rest vals))))
        (begin
          (sqlite:exec conn
                       "INSERT INTO result (run_id, form_hash, tier, status, reason) VALUES (?1,?2,?3,?4,?5)"
                       [run-id h (keyword "*") :diverge
                        (render-pairs pass-pairs)])
          (concat statuses [:diverge]))
        statuses))))

(defn record-form-result [conn run-id origin file idx form thunk dumps]
  (let [msg (scan-msg form)]
    (record-thunk conn run-id origin file idx (string form) (if msg msg "")
                  (fn [tk o e] (exec-thunk-capture tk thunk o e)) dumps
                  active-tiers true)))

# Iterate the [idx thunk] entries from compile/barrier-module. `forms` is the
# file's source forms (unevaluated, epoch-dropped) indexed the same way, so
# entry idx → forms[idx] supplies each test form's label/hash/src. def/var
# setup forms produce no entry (they ran eagerly during the compile pass).
(defn process-entries [conn run-id origin file forms entries dumps acc]
  (if (empty? entries)
    acc
    (let [e (first entries)
          idx (get e 0)
          thunk (get e 1)
          form (get forms idx)
          statuses (record-form-result conn run-id origin file idx form thunk
                                       dumps)]
      (process-entries conn run-id origin file forms (rest entries) dumps
                       (concat acc statuses)))))

# A file that won't compile (or whose setup faults) has no test forms to run:
# record ONE file-level failure (a `vm` row joined to a synthetic form row whose
# `file` is the offending file, so SQL selection by file still finds it).
(defn record-file-error [conn run-id origin file payload dumps]
  (let [msg (field-str payload :message)
        h (string (hash (string "file-error:" file)))]
    (sqlite:exec conn
                 "INSERT OR IGNORE INTO form (hash, origin, file, form_index, label, src) VALUES (?1,?2,?3,?4,?5,?6)"
                 [h (string origin) (string file) -1 "file-level error"
                  (if msg msg "")])
    (sqlite:exec conn
                 "INSERT INTO result (run_id, form_hash, tier, status, reason, signal) VALUES (?1,?2,?3,?4,?5,?6)"
                 [run-id h :vm :fail msg (sig-of payload)])  # Attach whatever artifacts compiled (a non-compiling file often still
    # parses to an `ast`), so even a file-level failure has a queryable record.
    (insert-assets conn
                   (get (get (sqlite:query conn
                             "SELECT last_insert_rowid() AS id") 0) :id) dumps)
    [:fail]))

# A file whose eager SHARED SETUP raised a loud `(gate! …)` (`:gated`) — e.g. an
# optional FFI library that wouldn't load, re-raised as :gated at its import
# site. The compile aborts before any test thunk is built, so there are no
# per-form results to record; we mirror record-file-error but as a SKIP (the
# dependency is absent, not broken). One file-level row (form_index -1), counted
# in n_skip, leaves the gate exit at 0. See docs/test-runner.md § Gating.
(defn record-file-gated [conn run-id origin file payload dumps]
  (let [reason (field-str payload :reason)
        h (string (hash (string "file-gated:" file)))]
    (sqlite:exec conn
                 "INSERT OR IGNORE INTO form (hash, origin, file, form_index, label, src) VALUES (?1,?2,?3,?4,?5,?6)"
                 [h (string origin) (string file) -1 "file-level gated"
                  (if reason reason "")])
    (sqlite:exec conn
                 "INSERT INTO result (run_id, form_hash, tier, status, reason, signal) VALUES (?1,?2,?3,?4,?5,?6)"
                 [run-id h :vm :skip reason ":gated"])
    (insert-assets conn
                   (get (get (sqlite:query conn
                             "SELECT last_insert_rowid() AS id") 0) :id) dumps)
    [:skip]))

# The (elle/epoch N) declaration is file metadata, not a test — drop it.
(defn epoch-form? [f]
  (and (list? f) (> (length f) 0) (= (first f) (quote elle/epoch))))

(defn test-forms [src]
  (filter (fn [f] (not (epoch-form? f))) (read-all src)))

# Interpret a compile/{barrier,whole}-module result: ENTRIES (the [idx thunk]
# accumulator) on success → RUN-FN; a `:gated` shared-setup → one file-level
# SKIP; any other setup/compile fault → one file-level FAIL.
(defn dispatch-compiled [conn run-id origin file out dumps run-fn]
  (if (get out 0)
    (run-fn (get out 1))
    (if (= (get (get out 1) :error) :gated)
      (record-file-gated conn run-id origin file (get out 1) dumps)
      (record-file-error conn run-id origin file (get out 1) dumps))))

# A legacy multi-form file is one imperative script: compile it as a single
# whole-file thunk (compile/whole-module) and run that ONE thunk per tier, in
# source order, in isolation — matching a direct run. The per-form barrier (which
# hoists def/var eagerly ahead of the bare-expression test forms) reorders such a
# script (read-before-write) and re-runs shared mutations per tier; one thunk
# eliminates that. The file is its own form: src = the file, label = the first
# assert message anywhere in it. See docs/test-runner.md § Multi-form files.
(defn process-whole [conn run-id origin file name src forms dumps]  # Compile ONCE in the main VM to detect a compile error or a top-level :gated
  # (dispatch-compiled records the file-level error/skip row) — but DON'T run that
  # thunk. For execution we ship the file's parsed SYNTAX to a worker that
  # compiles + runs it with its own stdlib (exec-source-capture), so a file whose
  # forms `import` a yielding module (sync/redis/http2/process/grpc/subprocess)
  # shares one scheduler with the worker's ev/run. read-forms is sendable syntax.
  (let [out (protect (compile/whole-module src name))]
    (dispatch-compiled conn run-id origin file out dumps
                       (fn [entries]
                         (let [msg (scan-children forms)
                               read-forms (compile/read-forms src name)]
                           (record-thunk conn run-id origin file 0 src
                           (if msg msg "")
                           (fn [tk o e]
                             (exec-source-capture tk read-forms name o e)) dumps
                           whole-file-policies false))))))

# Compile SRC and run its test forms per tier. A single-form file/snippet (the
# durable corpus shape) uses the per-form barrier (compile/barrier-module); a
# legacy MULTI-form file is wrapped as one whole-file thunk (process-whole). A
# compile/setup error becomes one file-level failure.
(defn process-source [conn run-id origin file name src]
  (let [forms (test-forms src)
        dumps (capture-dumps src name)]
    (if (> (length forms) 1)
      (process-whole conn run-id origin file name src forms dumps)
      (dispatch-compiled conn run-id origin file
                         (protect (compile/barrier-module src name)) dumps
                         (fn [entries]
                           (process-entries conn run-id origin file forms
                           entries dumps []))))))

(defn process-file [conn run-id file]
  (process-source conn run-id file file file (slurp file)))

(defn process-eval [conn run-id expr]
  (process-source conn run-id ":adhoc" "<eval>" "<eval>" expr))

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

# ── result views (the "query forever" half: never hand-write SQLite) ──────
# A run is otherwise silent (the gate is the exit code, the record is the DB).
# These render it: print-summary after every run, --summary for an existing DB,
# --query for arbitrary SQL. All read the session DB; none re-run anything.

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

# Tally line + the problem rows (only when there are any). Tallies are computed
# live (count-status); a run without finished_at was KILLED mid-flight (OOM,
# signal — docs/test-runner.md § Run honesty) and is labelled so, because a
# partial all-pass result set must never read as green. To stderr, so it never
# mingles with --query's stdout or a test's captured output.
(defn print-summary [conn run-id]
  (let [meta (get (sqlite:query conn
                                "SELECT (finished_at IS NULL) AS trunc, n_selected AS sel FROM run WHERE id = ?1"
                                [run-id]) 0)  # The DB is a SESSION: it accumulates every run. Show which run this is of
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
    (eprintln "elle test · run " run-id " of " nruns " · " np " pass · " ns
              " skip · " nf " fail · " nd " diverge · " nt " timeout")
    (if (> bad 0)
      (begin
        (eprintln bad " problem" (if (= bad 1) "" "s")
                  " (query the DB for full detail):")
        (print-problems conn run-id))
      nil)))

# Gate honesty at startup: if this session DB's latest run never completed,
# say so before starting a new one — the killed process could not report
# anything itself, and its absence of failures must not read as green.
(defn warn-if-truncated [conn]
  (let [rows (sqlite:query conn
                           "SELECT id AS id FROM run WHERE finished_at IS NULL AND id = (SELECT max(id) FROM run)")]
    (if (empty? rows)
      nil
      (begin
        (eprintln "warning: the previous run in this session DB was killed:")
        (print-summary conn (get (get rows 0) :id))))))

# --query SQL: run it, print each row, exit. --summary: the latest run's tally.
(defn run-query [conn sql]
  (each r in (sqlite:query conn sql)
    (println r)))

(defn latest-run-id [conn]
  (let [rows (sqlite:query conn "SELECT max(id) AS id FROM run")]
    (if (empty? rows) nil (get (get rows 0) :id))))

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

(def db
  (or (get opts :db)
      (string (or (get (sys/env) "ELLE_CACHE") "target") "/elle-tests.db")))

# The CAS lives beside the session DB: <dir-of-db>/cas/<hash> (docs § CAS asset
# capture). Created up front so cas-put can write into it. `scratch-dir` holds
# the per-(form × tier) stdout/stderr redirect files (written + slurped +
# deleted by each worker; the directory is disposable).
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

# n_selected lands at insert (everything else about the row is written at
# completion), so a killed run's row still says how much work was planned.
(sqlite:exec conn "INSERT INTO run (tiers, n_selected) VALUES (?1, ?2)"
             [(tiers-str active-tiers)
              (+ (length (get opts :paths)) (length (get opts :eval)))])
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
