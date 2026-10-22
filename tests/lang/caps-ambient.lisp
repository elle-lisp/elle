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
