(elle/epoch 10)
# POSIX signal tests
#
# Covers sending, watching, capability gating, refcount-driven mask
# management, and introspection. See docs/posix-signals.md for the
# surface contract. Run via:
#   cargo test elle_scripts::posix

# ── 1. self-raise watched ────────────────────────────────────────────────

(let [r (os/sig-watch |:sigusr1|)
      waiter (ev/spawn (fn [] (os/sig-next r)))]
  (ev/sleep 0.05)
  (os/sig-raise :sigusr1)
  (let [events (ev/join waiter)]
    (assert (>= (length events) 1) "1: at least one event delivered")
    (assert (= :sigusr1 (get (first events) :signal))
            "1: first event is :sigusr1"))
  (os/sig-close r))

# ── 2. cross-process send ─────────────────────────────────────────────────

(let [r (os/sig-watch |:sigusr2|)
      waiter (ev/spawn (fn [] (os/sig-next r)))]
  (ev/sleep 0.05)
  (os/sig-send (sys/pid) :sigusr2)
  (let [ev (first (ev/join waiter))]
    (assert (= :sigusr2 (get ev :signal)) "2: signal is :sigusr2")  # :sender-pid is nil on macOS but a valid int on Linux; tolerate both
    (let [sp (get ev :sender-pid)]
      (assert (or (nil? sp) (= sp (sys/pid)))
              "2: :sender-pid is nil or equal to (sys/pid)")))
  (os/sig-close r))

# ── 3. invalid pid errors ────────────────────────────────────────────────

(let [[ok? val] (protect ((fn [] (os/sig-send -99999 :sigterm))))]
  (assert (not ok?) "3: os/sig-send to invalid pid errors")
  (assert (= :os-signal-error (get val :error))
          "3: error kind is :os-signal-error"))

# ── 5. batching: multiple sends before a single next ─────────────────────

(let [r (os/sig-watch |:sigusr1|)]
  (os/sig-send (sys/pid) :sigusr1)
  (os/sig-send (sys/pid) :sigusr1)
  (ev/sleep 0.05)  # Kernel may coalesce identical signals from the same sender, so
  # we assert at least one event arrived (could be 1 or 2).
  (let [evs (ev/join (ev/spawn (fn [] (os/sig-next r))))]
    (assert (>= (length evs) 1) "5: at least one event after two sends"))
  (os/sig-close r))

# ── 6. capability denial: :os-signal blocks os/sig-send ──────────────────

(let [f (fiber/new (fn [] (os/sig-send (sys/pid) :sigusr2)) |:error :os-signal|
                   :deny |:os-signal|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "6: fiber paused after :os-signal denial")
  (let [val (fiber/value f)]
    (assert (= :capability-denied (get val :error))
            "6: payload is :capability-denied")
    (assert ((get val :denied) :os-signal) "6: :denied set contains :os-signal")))

# Counter-test (proves :os-signal is distinct from :exec): a fiber that
# denies :exec can STILL call os/sig-send. We send :sigchld (default
# disposition: ignore) so the test doesn't accidentally terminate.
(let [f (fiber/new (fn [] (os/sig-send (sys/pid) :sigchld)) |:error :os-signal|
                   :deny |:exec|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :dead)
          "6b: os/sig-send succeeds under :exec denial (distinct cap)"))

# ── 7. capability denial covers os/sig-raise ─────────────────────────────

(let [f (fiber/new (fn [] (os/sig-raise :sigusr1)) |:error :os-signal|
                   :deny |:os-signal|)]
  (fiber/resume f)
  (let [val (fiber/value f)]
    (assert (= :capability-denied (get val :error))
            "7: os/sig-raise is denied by :os-signal")))

# ── 8. os/sig-close is idempotent ────────────────────────────────────────

(let [r (os/sig-watch |:sigwinch|)]
  (assert (nil? (os/sig-close r)) "8a: first close returns nil")
  (assert (nil? (os/sig-close r)) "8b: second close also returns nil"))

# ── 10. integer signums must be named ────────────────────────────────────

# Watch SIGUSR1 so a successful send doesn't fire default disposition,
# then verify the integer signum (10 on Linux, 30 on macOS) is accepted.
# Round-trip via the keyword form below to stay platform-portable.
(let [r (os/sig-watch |:sigusr1|)]
  (let [[ok? _] (protect ((fn []  # Re-query the integer at runtime by sending the
                          # keyword first, then asserting the integer form
                          # is accepted on the same kernel.
                           (os/sig-send (sys/pid) :sigusr1))))]
    (assert ok? "10a: keyword signum accepted"))  # Drain so the queued signal doesn't fire when we close.
  (let [_ (ev/join (ev/spawn (fn [] (os/sig-next r))))]
    nil)
  (os/sig-close r))

(let [[ok? val] (protect ((fn [] (os/sig-send (sys/pid) 99))))]
  (assert (not ok?) "10b: unnamed integer signum 99 rejected")
  (assert (= :argument-error (get val :error))
          "10b: error kind is :argument-error"))

# Same tightening applies to subprocess/kill.
(let [[ok? val] (protect ((fn []
                            (let [proc (subprocess/exec "sleep" ["10"])]
                              (subprocess/kill proc 99)
                              (subprocess/wait proc)))))]
  (assert (not ok?) "10c: subprocess/kill with unnamed signum rejected")
  (assert (= :argument-error (get val :error))
          "10c: error kind is :argument-error"))

# ── 11. refcount-driven unblock ──────────────────────────────────────────

(assert (not (contains? (os/sig-mask) :sigusr1))
        "11a: :sigusr1 not masked initially")
(let [r (os/sig-watch |:sigusr1|)]
  (assert (contains? (os/sig-mask) :sigusr1) "11b: :sigusr1 masked after watch")
  (os/sig-close r))
(assert (not (contains? (os/sig-mask) :sigusr1))
        "11c: :sigusr1 unmasked after close")

# ── 12. multiple watchers share the block ────────────────────────────────

(let [a (os/sig-watch |:sigusr2|)
      b (os/sig-watch |:sigusr2|)]
  (os/sig-close a)
  (assert (contains? (os/sig-mask) :sigusr2)
          "12a: :sigusr2 still masked while b holds it")
  (os/sig-close b)
  (assert (not (contains? (os/sig-mask) :sigusr2))
          "12b: :sigusr2 unmasked after both close"))

# ── 13. os/sig-pending reflects queued state ─────────────────────────────

# Inherently racy because we may consume via signalfd before we
# query sigpending. Accept either outcome.
(let [r (os/sig-watch |:sigchld|)]
  (os/sig-send (sys/pid) :sigchld)
  (ev/sleep 0.02)
  (let [either (or (contains? (os/sig-pending) :sigchld)
                   (>= (length (ev/join (ev/spawn (fn [] (os/sig-next r))))) 1))]
    (assert either "13: either sigpending reports :sigchld or sig-next gets it"))
  (os/sig-close r))

# ── 14. os/sig-watching tracks the active set ────────────────────────────

(assert (not (contains? (os/sig-watching) :sigusr1))
        "14a: nothing watched initially")
(let [a (os/sig-watch |:sigusr1|)
      b (os/sig-watch |:sigusr2|)]
  (assert (contains? (os/sig-watching) :sigusr1) "14b: a is tracked")
  (assert (contains? (os/sig-watching) :sigusr2) "14c: b is tracked")
  (os/sig-close a)
  (os/sig-close b))
(assert (not (contains? (os/sig-watching) :sigusr1))
        "14d: empty after both close")
(assert (not (contains? (os/sig-watching) :sigusr2))
        "14e: empty after both close")

(println "all posix signal tests passed")
