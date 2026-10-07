(elle/epoch 14)
# audited: 2026-10-05
## elle test — a file as its own process: the child's argv, the run to its end
## or its deadline, and the row its exit status becomes.
## docs/test-runner.md
##
## A fragment of one module (see store.lisp).

# A worker thread isolates a fault and shares the process, which is not enough
# for a mode the process sets once: --trace=guardfree reports a use-after-free
# as a SIGSEGV that would take the runner down with every result it had not
# written yet. `--isolate FLAGS` runs each path as its own child instead, and
# the child's exit status is the whole verdict. The child is this executable,
# so `elle-rig test` runs each path on the rig; `--host PROGRAM` names another
# program, and the run then has no build (docs/test-runner.md).

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

# Run one child to its end, or to the deadline, under this process's own
# environment. Returns {:status INT-OR-NIL :stdout S :stderr S :wall-ms N
# :cpu-us N :max-rss-kb N} — nil status means the budget ran out and the child
# was killed. The cost is spawn to reap, and the child's total from
# subprocess/rusage (docs/test-store.md). A deadline can land in the moment a
# wait has already reaped the child, so the recorded status is consulted before
# the timeout is believed; a reap is kept on the subprocess, never spent
# (docs/subprocess.md).
#
# The child is THIS binary unless `--host` names another, never whatever `elle` a
# PATH lookup finds: a run has to say something about the build under test, and
# a different build would make it say nothing. (sys/argv) cannot answer — under
# a subcommand its head is the subcommand's own source name — so the binary
# reports its own path.
(defn run-child [argv budget-ms]
  (let [t0 (clock/monotonic)
        child (subprocess/exec (if host-program host-program (elle/executable))
                               argv {:stdin :null})
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
    (let [wall (ms-since t0)
          u (subprocess/rusage child)]
      (struct :status status :stdout (ev/join out-f) :stderr (ev/join err-f)
              :wall-ms wall
              :cpu-us (if u (+ (get u :user-us) (get u :sys-us)) nil)
              :max-rss-kb (if u (get u :max-rss-kb) nil)))))

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
