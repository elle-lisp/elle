(elle/epoch 15)
# audited: 2026-10-22
# Denying :ambient stops every read of process state, and leaves computation over a fiber's own values open.
# docs/signals/capabilities.md
#
# Before :ambient, the environment, the pid and the clocks reached a fiber
# through no capability bit, so no :deny set closed them. The counter-factual
# is an ambient primitive that declares no :ambient: its call runs, and the
# fiber finishes where it should have parked.

(defn ambient-denial [body]
  "The denial body parks on, run in a fiber denied :ambient, or nil when it
   runs to its end."
  (let [f (fiber/new body |:ambient :error| :deny |:ambient|)]
    (fiber/resume f)
    (when (= (fiber/status f) :paused) (fiber/value f))))

(defn assert-denied [body primitive]
  "Assert that body, denied :ambient, parks on the denial of primitive."
  (let [d (ambient-denial body)]
    (assert (= (get d :error) :capability-denied)
            (string primitive " is denied under :deny |:ambient|"))
    (assert (= (get d :primitive) primitive)
            (string "the denial names " primitive))
    (assert (= (get d :denied) |:ambient|)
            (string primitive " is denied :ambient"))))

# ── The bit ───────────────────────────────────────────────────────────

(assert (= (get (signals) :ambient) 19) "the registry names the ambient bit")
(assert ((fiber/caps) :ambient) "the root fiber holds :ambient")
(let [f (fiber/new (fn [] 42) |:error| :deny |:ambient|)]
  (assert (not ((fiber/caps f) :ambient))
          "fiber/caps omits :ambient when denied")
  (assert ((fiber/caps f) :io) ":deny |:ambient| leaves :io held"))

# ── A denied fiber reads no process state ─────────────────────────────

(assert-denied (fn [] (sys/env "HOME")) "sys/env")
(assert-denied (fn [] (sys/args)) "sys/args")
(assert-denied (fn [] (sys/pid)) "sys/pid")
(assert-denied (fn [] (clock/monotonic)) "clock/monotonic")
(assert-denied (fn [] (clock/realtime)) "clock/realtime")
(assert-denied (fn [] (vm/config :path)) "vm/config")
(assert-denied (fn [] (hash "x")) "hash")
(assert-denied (fn [] (port/stdout)) "port/stdout")

# ── Computation over its own values runs ──────────────────────────────

(let [f (fiber/new (fn [] (string/join (map string [1 2 3]) "-")) |:error|
                   :deny |:ambient|)]
  (assert (= (fiber/resume f) "1-2-3") "a denied fiber computes from its values")
  (assert (= (fiber/status f) :dead) "and finishes"))

# ── A fiber a sandbox spawns keeps the denial ─────────────────────────

(defn refused? [outcome]
  "Whether a protect outcome is the refusal of sys/pid, denied :ambient."
  (let [[ok? denial] outcome]
    (and (not ok?) (= (get denial :error) :capability-denied)
         (= (get denial :primitive) "sys/pid")
         (= (get denial :denied) |:ambient|))))

# The scheduler refuses the denial at the spawned fiber's own call site. The
# counter-factual: an ev/spawn mask that does not name :ambient, so the denial
# passes the scheduler and fails the file.
(let [f (fiber/new (fn [] (ev/join (ev/spawn (fn [] (protect (sys/pid))))))
                   |:error| :deny |:ambient|)]
  (assert (refused? (fiber/resume f)) "a spawned fiber is refused its pid read"))

# A process system started in the sandbox: its scheduler refuses the denial, so
# the process fiber's mask must name :ambient. The sandbox catches :ambient, so
# a denial that passed the process scheduler stops it there.
(def process ((import "std/process")))
(let [out @[]
      sandbox (fiber/new (fn []
                           (process:start (fn [] (push out (protect (sys/pid))))))
                         |:error :ambient| :deny |:ambient|)]
  (fiber/resume sandbox)
  (assert (= (fiber/status sandbox) :dead)
          "the sandboxed process system finishes")
  (assert (refused? (get out 0)) "a process is refused its pid read"))

# ── A macro body may mediate the denial ───────────────────────────────

# A fiber in the body whose mask names :ambient catches the denial, as any
# parent does, and the body expands to what its own code chose. The pid is
# never read. The counter-factual: a refusal no mask catches, and the compile
# failed.
(defmacro mediated []
  (let [f (fiber/new (fn [] (sys/pid)) |:ambient|)
        denial (fiber/resume f)]
    (string (get denial :primitive) " " (get denial :error))))
(assert (= (mediated) "sys/pid capability-denied")
        "a fiber in the body catches the denial")

# ── The bit is in the signal a call infers ────────────────────────────

(defn compiles? [src]
  "Whether src compiles. Nothing in it runs, so a refusal is the compiler's."
  (first (protect (compile/whole-module src "<ambient>"))))

(let [sig (compile/signal (compile/analyze "(defn f [] (sys/pid))") :f)]
  (assert (contains? (get sig :bits) :ambient)
          "a function that reads the pid carries :ambient")
  (assert (not (get sig :silent)) "so it is not silent"))

(assert (compiles? "(defn f [] (sys/pid))") "the body alone compiles")
(assert (not (compiles? "(defn f [] (silent!) (sys/pid))"))
        "silent! refuses a body that reads the pid")

# ── What a reader of the primitive learns ─────────────────────────────

(assert (contains? (get (vm/primitive-meta "sys/env") :bits) :ambient)
        "vm/primitive-meta names :ambient among the bits sys/env declares")
(assert (not (contains? (get (vm/primitive-meta "string/join") :bits) :ambient))
        "and not among those string/join declares")

(println "caps-ambient: OK")
