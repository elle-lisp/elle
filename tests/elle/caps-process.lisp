(elle/epoch 13)
# audited: 2026-09-29
# ── A process keeps its spawner's denial ──────────────────────────────
#
# `process:spawn`, `spawn-link` and `spawn-monitor` create the new process's
# fiber in the fiber that calls them, so the process carries that fiber's
# withheld set. The process scheduler refuses a denied call from a process and
# from a sub-fiber a process spawns (docs/process-scheduler.md).
#
# Counterfactual: when the scheduler creates the process fiber itself, from a
# command naming the closure, the process takes the scheduler's authority and
# the write lands. A sandbox inside a process escapes through any spawn.
#
# Each sandbox here runs inside a process and denies :fs. Its mask names only
# :error, so the spawn command it yields travels up to the process scheduler
# like any command the process itself yields.

(def process ((import "std/process")))

(defn refused? [outcome primitive]
  "Whether a protect outcome is the refusal of `primitive`."
  (let [[ok? denial] outcome]
    (and (not ok?) (= (get denial :error) :capability-denied)
         (= (get denial :primitive) primitive)
         (contains? (get denial :denied) :fs))))

(defn from-sandbox [spawner body]
  "Inside a process, spawn body through spawner from a sandbox denied :fs, and
   return the first message the spawned process sends back."
  (let [@got nil]
    (process:start (fn []
                     (let* [me (process:self)
                            sandbox (fiber/new (fn []
                              (spawner (fn [] (process:send me (body)))))
                            |:error| :deny |:fs|)]
                       (fiber/resume sandbox)
                       (assign got (process:recv)))))
    got))

# ── The spawned process holds its spawner's set ───────────────────────

(let [caps (from-sandbox process:spawn (fn [] (fiber/caps)))]
  (assert (not (contains? caps :fs))
          "a process spawned from the sandbox lacks :fs")
  (assert (contains? caps :io) "and keeps what nobody withheld"))

# ── Its denied call is refused at its own call site ───────────────────

(with-temp-dir dir
               (let [target (path/join dir "spawn")]
                 (assert (refused? (from-sandbox process:spawn
                                   (fn [] (protect (file/write target "x"))))
                                   "file/write")
                         "process:spawn: the write is refused where the process made it")
                 (assert (not (path/exists? target))
                         "and nothing reaches the disk")))

(with-temp-dir dir
               (let [target (path/join dir "link")]
                 (assert (refused? (from-sandbox process:spawn-link
                                   (fn [] (protect (file/write target "x"))))
                                   "file/write")
                         "process:spawn-link carries the denial too")
                 (assert (not (path/exists? target))
                         "and nothing reaches the disk")))

# spawn-monitor answers [pid ref], so the sandbox's spawner discards the ref.
(with-temp-dir dir
               (let [target (path/join dir "monitor")]
                 (assert (refused? (from-sandbox (fn [body]
                                     (first (process:spawn-monitor body)))
                                   (fn [] (protect (file/write target "x"))))
                                   "file/write")
                         "process:spawn-monitor carries the denial too")
                 (assert (not (path/exists? target))
                         "and nothing reaches the disk")))

# ── An uncaught refusal ends the process with the denial ──────────────

(with-temp-dir dir
               (let [target (path/join dir "uncaught")
                     @down nil]
                 (process:start (fn []
                                  (let [sandbox (fiber/new (fn []
                                          (process:spawn-monitor (fn []
                                            (file/write target "x")))) |:error|
                                        :deny |:fs|)]
                                    (fiber/resume sandbox)
                                    (assign down (process:recv)))))
                 (let [[tag _ref _pid reason] down]
                   (assert (= tag :DOWN)
                           "the monitor reports the process's exit")
                   (assert (= (first reason) :error)
                           "the process exited on an error")
                   (assert (= (get (get reason 1) :primitive) "file/write")
                           "and the error is the refused write's denial"))
                 (assert (not (path/exists? target))
                         "and nothing reaches the disk")))

# ── A sub-fiber a sandbox spawns in a process is refused too ──────────

(with-temp-dir dir
               (let [target (path/join dir "sub")
                     @outcome nil]
                 (process:start (fn []
                                  (let [sandbox (fiber/new (fn []
                                          (protect (ev/join (ev/spawn (fn []
                                            (file/write target "x"))))))
                                        |:error| :deny |:fs|)]
                                    (assign outcome (fiber/resume sandbox)))))
                 (assert (refused? outcome "file/write")
                         "the sub-fiber's write is refused, and the join raises the denial")
                 (assert (not (path/exists? target))
                         "and nothing reaches the disk")))

# ── A process system started inside a sandbox ─────────────────────────

# Every process and sub-fiber here is created inside the sandbox, so the
# scheduler refuses the denial itself. The sandbox's :error-only mask means a
# denial that escaped the scheduler instead would end this file at the root.
(with-temp-dir dir
               (let [target (path/join dir "inner")
                     out @[]
                     sandbox (fiber/new (fn []
                                          (process:start (fn []
                                            (push out
                                            (protect (file/write target "x"))))))
                                        |:error| :deny |:fs|)]
                 (fiber/resume sandbox)
                 (assert (= (fiber/status sandbox) :dead)
                         "the sandboxed process system finishes")
                 (assert (refused? (get out 0) "file/write")
                         "a process in a sandboxed system is refused by its own scheduler")
                 (assert (not (path/exists? target))
                         "and nothing reaches the disk")))

(println "caps-process: OK")
