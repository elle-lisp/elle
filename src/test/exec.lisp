(elle/epoch 13)
# audited: 2026-09-30
## elle test — running one test: worker isolation, output capture, the
## per-form deadline, the build's tiers, and a file's own process.
## docs/test-runner.md
##
## A fragment of one module (see store.lisp).
##
## Execution core: a single-form file compiles once via (compile/barrier-module
## SRC NAME) — the real file-compilation path (epoch + whole-module analysis:
## shared bindings, forward references, capture/signal inference). It runs the
## file's def/var forms eagerly and hands back the test form as a 0-arg THUNK,
## which runs on each tier via (compile/run-on TIER thunk) under (protect ...)
## in a worker (exec-thunk-capture). A multi-form file runs as one whole-file
## thunk instead: its syntax goes to a worker that compiles and runs it under
## each JIT policy (exec-source-capture). Both keep the *typed* failure signal
## (for example {:error :failed-assertion ...}), with no eval stringification.
## Only an --isolate run reads a child's exit status and stderr (run-child).
##
## stdout/stderr capture: each tier run executes under a worker-side (ev/run
## ...), with *stdout*/*stderr* rebound to temp files; non-empty output becomes
## `stdout`/`stderr` assets per (form × tier). See exec-thunk-capture.
##
## Per-test timeout: the budget the form's path earned (--timeout MS, default
## 60000, or --wide-timeout for a path --wide names) bounds each form's worker
## via os/join's deadline; an over-budget form is recorded `timeout` and gates
## non-zero. `--trace=KW` is split off by the `test` subcommand and applied to
## the runner's VM/free-log (for example `--trace=free` to attribute a UAF);
## the runner itself does not interpret it.

# `exec-thunk` runs one thunk on a tier, fault-isolated: spawn a worker (own
# VM), force the closure onto the tier under `protect`, and `os/join` marshals
# back the structured [ok? payload]. The barrier lives OUTSIDE the tiered
# closure deliberately — a fiber-based catch INSIDE a closure handed to
# compile/run-on is rejected by the optimizing tiers (they cannot create the
# handler closure). This probe path joins with no deadline (its closures are
# trivial and always finish); real test forms run under exec-thunk-capture,
# which bounds the join with `form-budget` so a hung test is recorded
# `timeout`, not a wedge.
(defn exec-thunk [tier thunk]
  (os/join (os/spawn-vm (fn [] (protect (compile/run-on tier thunk))))))

# Worker kind for the per-form execution (exec-thunk-capture). Default is the
# LIGHT worker (sys/spawn-vm, primitives only), which skips the init_stdlib the
# heavy worker (sys/spawn) runs on every spawn: a single-form thunk captures its
# already-imported deps from the eager main-VM setup, so it needs no stdlib in
# the worker. Nothing in the runner binds this to true. A whole-file run needs
# the heavy worker, and exec-source-capture calls os/spawn itself.
(def *heavy-worker* (make-parameter false))
(defn worker-spawn [closure]
  (if (*heavy-worker*) (os/spawn closure) (os/spawn-vm closure)))

# The wall-clock budget bounding ONE form's worker, in ms. main.lisp binds it
# per path to what that path earned, because the budget is a property of the
# file a form came from rather than of the run (docs/test-cli.md). An ad-hoc
# `-e` form has no path to earn one, so nil here falls through to `--timeout`.
(def *form-budget-ms* (make-parameter nil))

(defn form-budget []
  (let [ms (*form-budget-ms*)]
    (if ms ms test-timeout-ms)))

# Test code is untrusted: it can corrupt VM state, loop, or exhaust resources.
# So each thunk runs in a worker with a VM of its own, under `protect`, and
# `os/join` marshals back the structured [ok? payload] (docs/test-runner.md).
# The join carries the form's budget, so a hung test is recorded `timeout`
# rather than wedging the run.
#
# A spawned worker has only primitives — no scheduler — so any async I/O the test
# does (println, port/open, sockets) would yield into the void. We give the worker
# a real runtime the cheap way: the closure we ship references `ev/run` (stdlib),
# so the serializer drags `ev/run`'s whole closure graph into the bundle. The
# worker runs the tiered call under that ev/run, with `*stdout*`/`*stderr*`
# rebound to temp files; it slurps and deletes them and marshals [result stdout
# stderr] back through os/join.
#
# Run the tiered call with *stdout*/*stderr* rebound to temp files, returning
# {:result [ok? payload] :stdout S :stderr S :gauges DIFFS}. DIFFS is each heap
# gauge's change across the tiered call alone (docs/test-gauges.md). Assumes a
# scheduler is running (port I/O yields): the worker supplies its own via
# ev/run; the in-process fallback relies on the runner's top-level ev/run.
(defn capture-run [tier thunk out-path err-path]
  (let [op (port/open out-path :write)
        ep (port/open err-path :write)]
    (sys/trap-exit! true)
    (let [[v diffs] (parameterize ((*stdout* op)
                                   (*stderr* ep))
                      (gauges-around (fn []
                                       (protect (compile/run-on tier thunk)))))]
      (sys/trap-exit! false)
      (port/close op)
      (port/close ep)
      (let [so (slurp out-path)
            se (slurp err-path)]
        (file/delete out-path)
        (file/delete err-path)
        (struct :result v :stdout so :stderr se :gauges diffs)))))

(defn last-output-line [text]
  "The last non-empty line of `text`, or nil when it has none. Long lines are
   cut so one problem row stays one row."
  (if (= text nil)
    nil
    (let [lines (filter (fn [l] (> (length l) 0)) (string/split text "\n"))]
      (if (empty? lines)
        nil
        (let [l (get lines (- (length lines) 1))]
          (if (> (length l) 200) (concat (slice l 0 200) "…") l))))))

# A timed-out worker is ABANDONED, not killed — `sys/join` says so, because an
# OS thread cannot be safely killed. So at the moment the deadline is declared
# the wedged thread is still in THIS process, parked in whatever call stopped
# it, and its stack is still readable. Photograph every thread here, while that
# is true: after the run the process exits and the only account left is which
# budget ran out.
#
# This is the difference between a stack and a guess for a form that hangs on
# one machine and nowhere else. Re-running the file cannot stand in for it —
# the runner puts each form on its own worker thread, so a hang that needs that
# thread never reproduces under a plain run of the same file.
#
# stderr, because a terminal-only reader (a CI log) is who needs it, and the
# problem list above is already too narrow to hold a backtrace. Best-effort by
# construction: a box with no sampler prints nothing and the run is unaffected.
# `$PPID` inside `sh` is this process. Both samplers bound their own runtime
# (`sample` by its duration argument), so neither can wedge the run that is
# already in trouble.
#
# The photograph is printed whole. The runner cannot tell which thread wedged,
# and a cut at any length drops the threads the sampler happens to list last.
(defn photograph-threads []
  "A native backtrace of every thread in this process, or nil when the box has
   no sampler. `sample` is macOS's and always present there; `eu-stack` covers
   a Linux box that has elfutils."
  (let [[ok? proc] (protect (subprocess/exec "sh"
                            ["-c"
                             "sample $PPID 2 2>/dev/null || eu-stack -p $PPID 2>/dev/null"]))]
    (when ok?
      (let [[read-ok? out] (protect (string (port/read-all (get proc :stdout))))]
        (protect (subprocess/wait proc))
        (when (and read-ok? (> (length out) 0)) out)))))

(defn jit-frame-addrs [shot]
  "The unique `[0x…]` addresses on the photograph's `???` lines — the JIT
   frames the code map attributes and `jit/peek` reads."
  (let [@out @[]]
    (each line in (string/split shot "\n")
      (when (string/contains? line "???")
        (let [i (string/find line "[0x")]
          (when i
            (let [j (string/find line "]" i)]
              (when j
                (push out (slice line (+ i 1) j))))))))
    (distinct out)))

(defn note-timeout-stacks [c]
  "Print the wedged process's threads when a form misses its deadline."
  (when (= (get c :status) :timeout)
    (let [[ok? shot] (protect (photograph-threads))]
      (when (and ok? shot)
        (eprintln "── threads at the deadline ──────────────────────────────")
        (eprintln shot)
        # The photograph cannot symbolize JIT frames (anonymous Cranelift
        # mappings). The registry is their symbol table — process-global, so
        # it covers the wedged worker's compiles too. A sampled `???` address
        # resolves to the nearest preceding entry (docs/impl/jit.md).
        (let [[map-ok? jit-map] (protect (vm/query "jit/map" nil))]
          (when (and map-ok? (string? jit-map) (> (length jit-map) 0))
            (eprintln "── jit code map (addr name; match ??? frames to the nearest preceding addr) ──")
            (eprintln jit-map)))
        # The words each sampled JIT frame is parked on. The map names the
        # frame's function; this shows the bytes its PC is executing, which
        # is what decides whether the wedge is IN the emitted code or in
        # what the core fetched (docs/impl/jit.md; on AArch64, `0x14000000` is a
        # branch to itself).
        (let [[a-ok? addrs] (protect (jit-frame-addrs shot))]
          (when (and a-ok? (> (length addrs) 0))
            (eprintln "── code around sampled jit frames (each line names its address) ──")
            (each a in (take 4 addrs)
              (let [[p-ok? words] (protect (vm/query "jit/peek" a))]
                (when (and p-ok? (string? words))
                  (eprintln (concat "pc " a ":"))
                  (eprintln words))))))
        (eprintln "── end threads ──────────────────────────────────────────"))))
  c)

# A timeout's reason names the budget that ran out — `join: deadline exceeded`
# — and says nothing about where the form was when it did. The last line the
# form printed says exactly that, so carry it in the reason: the problem list
# is what a terminal-only reader (a CI log) gets, and it should not need a
# query to name the call that hung. The full output stays in the assets.
(defn note-last-output [c cap]
  (if (= (get c :status) :timeout)
    (let [tail (let [e (last-output-line (get cap :stderr))]
                 (if e e (last-output-line (get cap :stdout))))]
      (if tail
        (put c :reason (concat (get c :reason) " · last output: " tail))
        c))
    c))

(defn slurp-partial [path]
  "What `path` holds, or an empty string when it holds nothing readable."
  (let [[ok? content] (protect (slurp path))]
    (if ok? content "")))

# Recover the output of a run that never came back.
#
# `capture-run` slurps and deletes the redirect files once the tiered call
# returns; a form killed by the join deadline never reaches that. Its output is
# the only account of which call it was in when the deadline struck, and the
# form wrote it before it wedged — so read the files here and record them
# against the timeout. Deleting them also keeps an abandoned worker from
# leaving its redirect pair in the scratch directory; the worker may still hold
# its end open, which POSIX allows.
(defn salvage-capture [result out-path err-path]
  (let [so (slurp-partial out-path)
        se (slurp-partial err-path)]
    (protect (file/delete out-path))
    (protect (file/delete err-path))
    (struct :result result :stdout so :stderr se)))

# A value that cannot serialize cannot cross between the main VM and a worker,
# in either direction (docs/test-runner.md). Both crossings raise :thread-error:
# the spawn, "spawn: Cannot send …", when the thunk captures one, and the join,
# "Failed to serialize result: …", when the form's value is one. The message
# test keeps a worker panic out of the unisolated in-process path, so a panic
# stays a fail rather than crashing the main VM.
(defn serialization-error? [payload]
  (and (= (get payload :error) :thread-error)
       (let [m (string (get payload :message))]
         (or (string/contains? m "send") (string/contains? m "serialize")))))

(defn exec-thunk-capture [tier thunk out-path err-path]
  (let [outcome (protect (os/join (worker-spawn (fn []
                                    (ev/run (fn []
                                      (capture-run tier thunk out-path err-path)))))
                                  (form-budget)))]
    (if (get outcome 0)
      (get outcome 1)
      # The worker spawn/join failed. If a value could not cross — the thunk
      # going out, or the form's value coming back — run the form IN-PROCESS:
      # no isolation, no timeout, but it runs (docs/test-runner.md). Any other
      # thread-error (for example a worker panic) stays a recorded fail.
      (if (serialization-error? (get outcome 1))
        # In-process runs share the MAIN VM, so a form that sets :trace (for
        # example to |:call|) and never clears it — an assert aborts first —
        # would leave the runner's own machinery traced.
        # Save and restore the main VM's trace around the run to contain it
        # (worker runs are already isolated by their fresh VM).
        (let [saved-trace (vm/config :trace)
              r (capture-run tier thunk out-path err-path)]
          (vm/config-set :trace saved-trace)
          r)
        (salvage-capture [false (get outcome 1)] out-path err-path)))))

# Run THUNK under EVRUN and capture its stdout/stderr. Elle has NO synchronous
# I/O — every port/socket/subprocess op yields an io-request — so the file's
# TOP-LEVEL I/O needs a running scheduler. The thunk runs as a fiber that
# `(spawn thunk)` adds to that scheduler and `(join …)` waits for.
# EVRUN/SPAWN/JOIN/OUT/ERR are passed so the caller supplies the SAME stdlib
# instance the thunk uses (the worker's, or the main VM's for the in-process
# fallback). Returns {:result [ok? value] :stdout :stderr :gauges}, the gauges
# bracketing the pumped run as capture-run's do.
(defn
  capture-pumped
  [evrun spawn join out-param err-param thunk out-path err-path]
  (evrun (fn []
           (let [op (port/open out-path :write)
                 ep (port/open err-path :write)]
             (sys/trap-exit! true)
             (let [[v diffs] (parameterize ((out-param op)
                               (err-param ep))
                               (gauges-around (fn []
                                 (protect (join (spawn thunk))))))]
               (sys/trap-exit! false)
               (port/close op)
               (port/close ep)
               (let [so (slurp out-path)
                     se (slurp err-path)]
                 (file/delete out-path)
                 (file/delete err-path)
                 (struct :result v :stdout so :stderr se :gauges diffs)))))))

# The setting that puts the JIT back where `(vm/config :jit)` read it: nil is
# off, 0 is eager, and a count is the threshold (JitPolicy::reading in
# src/config/policy.rs).
(defn jit-setting [reading]
  (if (nil? reading) :off (if (= reading 0) :eager reading)))

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
# import/eval), so os/spawn (not -vm). `policy` is the JIT policy (:off /
# :eager, see whole-file-policies): the worker sets it via (vm/config-set :jit
# policy) before running, so the SAME file runs under bytecode and under JIT.
# Only the process running `elle test` may set either (docs/test-runner.md).
# The worker's VM is fresh, so the policy is isolated; the in-process fallback
# saves and restores the main VM's policy around the run.
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
                                  (form-budget)))]
    (if (get outcome 0)
      (get outcome 1)
      # Unsendable RESULT (an orphan fiber, an io-request, …) can't cross back
      # through os/join. Fall back to running IN-PROCESS — no isolation, no
      # timeout — compiling the same syntax against the MAIN stdlib and running
      # under the runner's own ev/run + *stdout*/*stderr* (all main-consistent),
      # as exec-thunk-capture does for a value that cannot cross. The main VM's
      # JIT policy is set for the run and restored after (it is shared, not fresh).
      (if (serialization-error? (get outcome 1))
        (let [saved (jit-setting (vm/config :jit))
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
        (salvage-capture [false (get outcome 1)] out-path err-path)))))

# ── running a file as its own process ────────────────────────────────
# A worker thread isolates a fault and shares the process, which is not enough
# for a mode the process sets once: --trace=guardfree reports a use-after-free
# as a SIGSEGV that would take the runner down with every result it had not
# written yet. `--isolate FLAGS` runs each path as `elle FLAGS PATH` instead, and
# the child's exit status is the whole verdict. `--host PROGRAM` names the
# program each child runs, which is how the implementation suite reaches the rig
# (docs/test-runner.md).

# A child's argv: the words of FLAGS, then PATH. An empty FLAGS adds no word.
(defn child-argv [flags path]
  (concat (filter (fn [f] (> (length f) 0)) (string/split flags " ")) [path]))

# Read a child's stream to EOF. Spawned as a fiber per stream, so both drain
# while the wait runs: a child that fills a pipe buffer with nobody reading is
# stopped in a write, which would read as a hang against its own budget.
(defn drain [p]
  (if (= p nil)
    ""
    (let [[ok? b] (protect (port/read-all p))]
      (if ok? (string b) ""))))

# Run one child to its end, or to the deadline. Returns
# {:status INT-OR-NIL :stdout S :stderr S} — nil status means the budget ran
# out and the child was killed. A deadline can land in the moment a wait has
# already reaped the child, so the recorded status is consulted before the
# timeout is believed; a reap is kept on the subprocess, never spent
# (docs/subprocess.md).
#
# The child is THIS binary unless `--host` names another, never whatever `elle` a
# PATH lookup finds: a run has to say something about the build under test, and
# a different build would make it say nothing. (sys/argv) cannot answer — under
# a subcommand its head is the subcommand's own source name — so the binary
# reports its own path.
(defn run-child [argv budget-ms env]
  (let [child (subprocess/exec (if host-program host-program (elle/executable))
                               argv {:stdin :null :env env})
        out-f (ev/spawn (fn [] (drain (get child :stdout))))
        err-f (ev/spawn (fn [] (drain (get child :stderr))))
        waited (ev/timeout (/ (float budget-ms) 1000.0)
                           (fn [] (subprocess/wait child)))
        status (if (= waited nil) (subprocess/exit child) waited)]
    (if (= status nil)
      (begin
        (subprocess/kill child :sigkill)
        (protect (subprocess/wait child)))
      nil)
    (struct :status status :stdout (ev/join out-f) :stderr (ev/join err-f))))

# What a terminating status says, rendered for a reader. A signalled child
# answers its signal number negated (docs/subprocess.md), so the sign is what
# separates the two cases and no second read is needed. os/sig-name covers the
# fault set that os/sig-send refuses, which is where a child's death lives.
(defn signal-note [n]
  (let [kw (os/sig-name n)]
    (if kw
      (struct :sig (string ":" (string kw))
              :reason (string "killed by " (string/uppercase (string kw))
                              " (signal " n ")"))
      (struct :sig ":signal" :reason (string "killed by signal " n)))))

# The line the binary prints when a loud gate refuses to run a file. A gated
# child exits 0, so its exit status alone would read as a vacuous pass — the
# coverage-hiding failure gate! exists to prevent. src/program.rs writes it.
(def gated-marker "SKIP (gated): ")

(defn gated-reason [text]
  (let [i (string/find text gated-marker)]
    (if i
      (let [rest (slice text (+ i (length gated-marker)) (length text))
            end (string/find rest "\n")]
        (if end (slice rest 0 end) rest))
      nil)))

# Classify a finished child into the same row shape a form's outcome takes
# (see classify). The child is gone, so there is nothing left to read but what
# it left behind: its status, and what it printed on the way.
(defn classify-child [cap budget-ms]
  (let [status (get cap :status)]
    (if (= status nil)
      (struct :status :timeout
              :reason (string "child exceeded the " budget-ms " ms budget"))
      (if (= status 0)
        (let [reason (gated-reason (get cap :stderr))]
          (if reason
            (struct :status :skip :sig ":gated" :reason reason)
            (struct :status :pass)))
        (if (< status 0)
          (let [note (signal-note (- 0 status))]
            (struct :status :fail :sig (get note :sig)
                    :reason (get note :reason)))
          (struct :status :fail :reason (string "exit " status)))))))

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
# every function, recorded "jit") — set per-worker via (vm/config-set :jit …).
# :eager is included only when this build carries the JIT. Each entry is
# [policy label].
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
