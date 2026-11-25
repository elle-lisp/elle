(elle/epoch 18)
# audited: 2026-11-23
## elle test — running one test: worker isolation, output capture and the
## per-form deadline. What a form that misses its deadline leaves is
## deadline.lisp's, and the tiers it runs on are tiers.lisp's.
## docs/test-runner.md
##
## A fragment of one module (see store.lisp).
##
## Execution core: a single-form file compiles once via (compile/barrier-module
## SRC NAME) — the real file-compilation path (epoch + whole-module analysis:
## shared bindings, capture/signal inference). It runs the
## file's def/var forms eagerly and hands back the test form as a 0-arg THUNK,
## which runs on each tier via (compile/run-on TIER thunk) under (protect ...)
## in a worker (exec-thunk-capture). A multi-form file runs as one whole-file
## thunk instead: its syntax goes to a worker that compiles and runs it under
## each JIT policy (exec-source-capture). Both keep the *typed* failure signal
## (for example {:error :failed-assertion ...}), with no eval stringification.
## Only an --isolate run reads a child's exit status and stderr (child.lisp).
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
# compile/run-on builds a handler closure, and a tier that cannot build one
# refuses the whole form. This probe path joins with no deadline, because its
# closures are trivial and always finish. Real test forms run under
# exec-thunk-capture, which bounds the join with `form-budget`, so a hung test
# is recorded `timeout`, not a wedge.
(defn exec-thunk [tier thunk]
  (os/join (os/spawn-vm (fn [] (protect (compile/run-on tier thunk))))))

# Worker kind for the per-form execution (exec-thunk-capture). Default is the
# LIGHT worker (sys/spawn-vm, primitives only). A single-form thunk captures
# its already-imported deps from the eager main-VM setup, so it needs no stdlib
# in the worker. Nothing in the runner binds this to true. A whole-file run
# needs the heavy worker, and exec-source-capture calls os/spawn itself.
(def *heavy-worker* (make-parameter false))
(defn worker-spawn [closure]
  (if (*heavy-worker*) (os/spawn closure) (os/spawn-vm closure)))

# The wall-clock budget bounding ONE form's worker, in ms. main.lisp binds it
# per path to what that path earned. The budget is a property of the file a
# form came from rather than of the run (docs/test-cli.md). An ad-hoc
# `-e` form has no path to earn one, so nil here falls through to `--timeout`.
(def *form-budget-ms* (make-parameter nil))

(defn form-budget []
  (let [ms (*form-budget-ms*)]
    (if ms ms test-timeout-ms)))

# What a result cost (docs/test-cost.md). (clock/cpu) is
# the calling thread's CPU time, so a delta read on the thread that ran the
# form leaves out the JIT's compile thread, the I/O pool and any child.
(defn cpu-us-since [t0]
  "Microseconds of this thread's CPU time since T0, a (clock/cpu) reading."
  (integer (* 1000000.0 (- (clock/cpu) t0))))

(defn ms-since [t0]
  "Milliseconds of wall time since T0, a (clock/monotonic) reading."
  (integer (* 1000.0 (- (clock/monotonic) t0))))

(defn us-since [t0]
  "Microseconds of wall time since T0, a (clock/monotonic) reading."
  (integer (* 1000000.0 (- (clock/monotonic) t0))))

# The rig's `jit/stats`, or nil under `elle`, which carries no poll
# (rig/overview.md). Read through `eval` once, as main.lisp reads the build:
# a user build has no such primitive, and the runner compiles under both. The
# value crosses into a worker, where it reads the worker's VM.
(def jit-stats
  (let [[ok? f] (protect (eval 'jit/stats))]
    (if ok? f nil)))

(defn jit-reading []
  "This VM's (jit/stats), or nil where the runner has none."
  (if jit-stats (jit-stats) nil))

(defn jit-since [j0]
  "The JIT compiles this VM took since J0, a jit-reading, and their cost, as
   {:compiles :rejected :wall-us :cpu-us}, or nil when J0 is nil. A compile
   still in flight counts in neither reading (docs/test-cost.md)."
  (when j0
    (let [j (jit-stats)]
      {:compiles (- j:compiles j0:compiles)
       :rejected (- j:rejected j0:rejected)
       :wall-us (- j:wall-us j0:wall-us)
       :cpu-us (- j:cpu-us j0:cpu-us)})))

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
# {:result [ok? payload] :stdout S :stderr S :cpu-us N :jit J :gauges READINGS}.
# J is jit-since over the window the CPU clock reads. READINGS are the heap
# gauges read around the tiered call alone (gauges-around), so the CPU clock
# and the JIT reading fall outside the gauges' window. Assumes a
# scheduler is running (port I/O yields): the worker supplies its own via
# ev/run; the in-process fallback relies on the runner's top-level ev/run.
(defn capture-run [tier thunk out-path err-path]
  (let [op (port/open out-path :write)
        ep (port/open err-path :write)]
    (sys/trap-exit! true)
    (let [j0 (jit-reading)
          t0 (clock/cpu)
          [v readings] (parameterize ((*stdout* op)
                                      (*stderr* ep))
                         (gauges-around (fn []
                                          (protect (compile/run-on tier thunk)))))
          cpu (cpu-us-since t0)
          jit (jit-since j0)]
      (sys/trap-exit! false)
      (port/close op)
      (port/close ep)
      (let [so (slurp out-path)
            se (slurp err-path)]
        (file/delete out-path)
        (file/delete err-path)
        (struct :result v :stdout so :stderr se :cpu-us cpu :jit jit
                :gauges readings)))))

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
  (let [outcome (protect (os/join (worker-spawn
                                    (fn []
                                      (ev/run
                                        (fn []
                                          (capture-run tier thunk out-path
                                            err-path)))))
                                  :timeout (if-let [ms (form-budget)]
                                             (/ ms 1000.0)
                                             nil)))]
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
# fallback).
#
# Returns {:result [ok? value] :stdout :stderr :cpu-us :jit :gauges}. The
# fibers the script spawns run on this thread's scheduler, so its CPU time
# counts them. The JIT reading and the gauges bracket the pumped run as
# capture-run's do.
(defn
  capture-pumped
  [evrun spawn join out-param err-param thunk out-path err-path]
  (evrun (fn []
           (let [op (port/open out-path :write)
                 ep (port/open err-path :write)]
             (sys/trap-exit! true)
             (let [j0 (jit-reading)
                   t0 (clock/cpu)
                   [v readings] (parameterize ((out-param op)
                                               (err-param ep))
                                  (gauges-around
                                    (fn [] (protect (join (spawn thunk))))))
                   cpu (cpu-us-since t0)
                   jit (jit-since j0)]
               (sys/trap-exit! false)
               (port/close op)
               (port/close ep)
               (let [so (slurp out-path)
                     se (slurp err-path)]
                 (file/delete out-path)
                 (file/delete err-path)
                 (struct :result v :stdout so :stderr se :cpu-us cpu :jit jit
                         :gauges readings)))))))

# The setting that puts the JIT back where `(vm/config :jit)` read it: nil is
# off, 0 is eager, and a count is the threshold (src/config/policy.rs).
(defn jit-setting [reading]
  (if (nil? reading) :off (if (= reading 0) :eager reading)))

# What a whole file's compile cost its run (docs/test-cost.md): the wall time
# and this thread's CPU time, and the heap gauges around the compile alone.
(defn compile-whole-file [forms name]
  "Compile the file's FORMS into its one thunk, and answer [thunk cost]. COST
   holds :compile-wall-us, :compile-cpu-us and :compile-gauges."
  (let [t0 (clock/monotonic)
        c0 (clock/cpu)
        [thunk readings] (gauges-around (fn []
                                          (get
                                            (get
                                              (compile/whole-module-syntax forms
                                                name) 0) 1)))
        cpu (cpu-us-since c0)]
    [thunk
     {:compile-wall-us (us-since t0)
      :compile-cpu-us cpu
      :compile-gauges readings}]))

# Whole-file (legacy multi-form) execution. Unlike exec-thunk-capture — which
# ships a MAIN-compiled thunk — this ships the file's parsed SYNTAX (sendable via
# os/spawn). The worker compiles it with compile/whole-module-syntax against
# its OWN stdlib, then runs it (pumped) under the WORKER's ev/run, and hands
# back what the compile cost with the run's capture.
#
# That is what makes the file's runtime `import`s and the scheduler agree on
# dynamic params. A main-compiled thunk binds `ev/spawn`/`*spawn*` to the MAIN
# stdlib, but a module the thunk `import`s at runtime resolves to the WORKER
# stdlib. Those are two distinct *spawn* parameter objects, so any file whose
# forms import a module that yields (sync/redis/http2/process/grpc) breaks.
# The worker resolves ev/run, *stdout* and *stderr* itself (eval) for the same
# reason: they must be the worker's parameter objects.
#
# Syntax compiles in the heavy worker (it runs the file's own import/eval), so
# os/spawn (not -vm). `policy` is the JIT policy (:off / :eager, see
# whole-file-policies). The worker sets it via (vm/config-set :jit policy)
# before running, so the SAME file runs under bytecode and under JIT. Only the
# process running `elle test` may set either (docs/test-runner.md). The
# worker's VM is fresh, so the policy is isolated; the in-process fallback
# saves and restores the main VM's policy around the run.
(defn exec-source-capture [policy forms name out-path err-path]
  (let [outcome (protect (os/join (os/spawn
                                    (fn []
                                      (vm/config-set :jit policy)
                                      (let [w-evrun (eval (quote ev/run))
                                            w-spawn (eval (quote ev/spawn))
                                            w-join (eval (quote ev/join))
                                            w-out (eval (quote *stdout*))
                                            w-err (eval (quote *stderr*))
                                            [thunk cost] (compile-whole-file
                                                           forms name)]
                                        (merge
                                          (capture-pumped w-evrun w-spawn w-join
                                            w-out w-err thunk out-path err-path)
                                          cost))))
                                  :timeout (if-let [ms (form-budget)]
                                             (/ ms 1000.0)
                                             nil)))]
    (if (get outcome 0)
      (get outcome 1)
      # Unsendable RESULT (an orphan fiber, an io-request, …) can't cross back
      # through os/join. Fall back to running IN-PROCESS, with no isolation and
      # no timeout, as exec-thunk-capture does for a value that cannot cross.
      # The run compiles the same syntax against the MAIN stdlib, under the
      # runner's own ev/run + *stdout*/*stderr* (all main-consistent). The main
      # VM's JIT policy is set for the run and restored after (it is shared,
      # not fresh).
      (if (serialization-error? (get outcome 1))
        (let [saved (jit-setting (vm/config :jit))
              saved-trace (vm/config :trace)
              [thunk cost] (compile-whole-file forms name)]
          (vm/config-set :jit policy)
          (let [r (merge (capture-pumped ev/run ev/spawn ev/join *stdout*
                           *stderr* thunk out-path err-path) cost)]
            (vm/config-set :jit saved)
            # Restore the main VM's trace too: a whole-file form that sets
            # :trace and aborts before clearing must not bleed into the runner.
            (vm/config-set :trace saved-trace)
            r))
        (salvage-capture [false (get outcome 1)] out-path err-path)))))
