(elle/epoch 12)
# ── Withheld capabilities cross a thread ───────────────────────────────
#
# `sys/spawn` and `sys/spawn-vm` deep-copy a closure into a FRESH VM with
# its own root fiber. That root fiber's withheld set is what the
# capability gate reads (`call_inner`: def.signal ∩ withheld ∩ CAP_MASK),
# so unless the spawning fiber's withheld travels with the closure, a
# sandboxed fiber escapes every denial by spawning a thread.
#
# The trap: `file/write` is synchronous. An earlier attempt to settle this
# with `:exec` saw `{:error :thread-error :message "Unexpected yield
# outside fiber context"}` and read it as enforcement. It was not — the
# subprocess primitive tried to suspend and found no fiber context. A
# synchronous primitive never hits that wall, so the escape was real and
# invisible.
#
# A worker has no parent to suspend into, so a denial there cannot be
# mediated. The thread ends and the join reports it.

(def root (file/mktempdir))

# ── The escape is closed ──────────────────────────────────────────────

# Counterfactual: without propagation the worker runs with an empty
# withheld set, the file appears on disk, and the join succeeds.
(defn spawn-write [spawner victim]
  (let [f (fiber/new (fn [] (spawner (fn [] (file/write victim "x"))))
                     |:fs :error| :deny |:fs|)]
    (fiber/resume f)))

(let [victim (path/join root "VIA-SPAWN-VM")
      handle (spawn-write sys/spawn-vm victim)
      [ok? err] (protect (sys/join handle))]
  (assert (not ok?) "the worker fails rather than writing")
  (assert (= :thread-error (get err :error)) "the join reports a thread error")
  (assert (not (path/exists? victim))
          "a thread cannot write what the fiber that spawned it may not"))

(let [victim (path/join root "VIA-SPAWN")
      handle (spawn-write sys/spawn victim)
      [ok? _err] (protect (sys/join handle))]
  (assert (not ok?) "the heavy worker fails too")
  (assert (not (path/exists? victim))
          "sys/spawn carries the denial exactly as sys/spawn-vm does"))

# ── The worker sees the denial, not just its effect ───────────────────

(let [f (fiber/new (fn [] (sys/spawn-vm (fn [] (fiber/caps)))) |:fs :error|
                   :deny |:fs|)
      caps (sys/join (fiber/resume f))]
  (assert (not (caps :fs)) "the worker's root fiber lacks :fs")
  (assert (caps :io) "and holds every capability that was not withheld"))

# ── An unrestricted fiber is untouched ────────────────────────────────

(let [allowed (path/join root "ALLOWED")
      f (fiber/new (fn [] (sys/spawn-vm (fn [] (file/write allowed "x"))))
                   |:fs :error|)]
  (sys/join (fiber/resume f))
  (assert (path/exists? allowed)
          "a fiber that denies nothing spawns a worker that writes"))

(let [allowed (path/join root "TOP-LEVEL")]
  (sys/join (sys/spawn-vm (fn [] (file/write allowed "x"))))
  (assert (path/exists? allowed) "the default path is untouched"))

(file/delete-dir-all root)
(println "caps-thread: OK")
