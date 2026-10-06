(elle/epoch 14)
# audited: 2026-10-06
# A fiber `ev/spawn` creates keeps its spawner's denial, and the scheduler refuses its denied calls.
# docs/signals/capabilities.md
#
# `ev/spawn` creates a fiber in the calling fiber and hands it to the scheduler,
# which resumes it. The fiber carries its creator's withheld set, and the
# scheduler refuses a denied call with the denial payload.
#
# The counter-factual, one per half. With the withheld set flowing only at
# resume, the spawned fiber takes the scheduler's empty set, so the write lands
# and `(fiber/caps)` lists :fs. With the inheritance but no refusal, a denial
# the spawned fiber's mask names falls to the scheduler's catch-all and is
# resumed with nil, which the fiber reads as the call's return: `protect`
# reports `[true nil]` for a subprocess that never ran.
#
# The sandbox's own mask names only :error. A denial that escapes the scheduler
# therefore reaches the root as an unhandled signal and fails the file, rather
# than landing quietly in a parent that happens to catch it.

(defn sandboxed [deny thunk]
  "Run thunk in a fiber denied `deny`, and return what it returns."
  (let [f (fiber/new thunk |:error| :deny deny)
        v (fiber/resume f)]
    (assert (= (fiber/status f) :dead) "the sandbox runs to completion")
    v))

(defn spawned [deny thunk]
  "Spawn thunk from inside a sandbox denied `deny`, and join it there."
  (sandboxed deny (fn [] (ev/join (ev/spawn thunk)))))

(defn refused? [outcome primitive bit]
  "Whether a protect outcome is the refusal of `primitive`, denied `bit`."
  (let [[ok? denial] outcome]
    (and (not ok?) (= (get denial :error) :capability-denied)
         (= (get denial :primitive) primitive)
         (contains? (get denial :denied) bit))))

# ── The spawned fiber holds its spawner's set ─────────────────────────

(let [caps (spawned |:fs| (fn [] (fiber/caps)))]
  (assert (not (contains? caps :fs)) "a fiber spawned in the sandbox lacks :fs")
  (assert (contains? caps :io) "and keeps what nobody withheld"))

(let [caps (spawned |:fs| (fn [] (ev/join (ev/spawn (fn [] (fiber/caps))))))]
  (assert (not (contains? caps :fs))
          "a fiber spawned by a spawned fiber lacks :fs too"))

# ── A denied call is refused at its own call site ─────────────────────

(with-temp-dir dir
               (let [target (path/join dir "caught")
                     outcome (spawned |:fs|
                                      (fn [] (protect (file/write target "x"))))]
                 (assert (refused? outcome "file/write" :fs)
                         "the spawned fiber's protect sees the refusal of its write")
                 (assert (not (path/exists? target))
                         "and nothing reaches the disk")))

(with-temp-dir dir
               (let [target (path/join dir "uncaught")
                     outcome (sandboxed |:fs|
                                        (fn []
                                          (protect (ev/join (ev/spawn (fn []
                                            (file/write target "x")))))))]
                 (assert (refused? outcome "file/write" :fs)
                         "an uncaught refusal ends the fiber, and the join raises the denial")
                 (assert (not (path/exists? target))
                         "and nothing reaches the disk")))

# ── A refused fiber runs on ───────────────────────────────────────────

# Refusal answers one call. A fiber that catches it reaches its next call,
# which is refused on its own account, and then returns its own value.
(with-temp-dir dir
               (let [a (path/join dir "a")
                     b (path/join dir "b")
                     [first-write second-write done] (spawned |:fs|
                     (fn []
                       [(protect (file/write a "x"))
                        (protect (file/write b "y")) :done]))]
                 (assert (refused? first-write "file/write" :fs)
                         "the first write is refused")
                 (assert (refused? second-write "file/write" :fs)
                         "the second is refused too")
                 (assert (= (get (get second-write 1) :args) [b "y"])
                         "the second refusal names the second call")
                 (assert (= done :done) "and the fiber returns its own value")))

# ── Every bit a denial can carry reaches the scheduler ────────────────

# The spawned fiber's mask must name each bit, or the denial propagates past
# the scheduler instead of being refused. :exec is the case that shows a
# refusal apart from a nil resume. tests/impl/caps-spawn-gpu.lisp holds the
# :gpu case, whose one gated call is an extension of this implementation.
(assert (refused? (spawned |:exec|
                           (fn []
                             (protect (subprocess/exec "/bin/sh" ["-c" "true"]))))
                  "subprocess/exec" :exec)
        "a denied subprocess is refused, not resumed with nil")

(assert (refused? (spawned |:os-signal|
                           (fn [] (protect (os/sig-send (sys/pid) :sigchld))))
                  "os/sig-send" :os-signal) "a denied signal send is refused")

# The gate refuses the call before the loader reads the path, so no library need
# exist. `import-file` hands a library name to `import/load-plugin`, and the
# payload names that primitive.
(assert (refused? (spawned |:ffi|
                           (fn [] (protect (import-file "nonexistent.so"))))
                  "import/load-plugin" :ffi) "a denied native import is refused")

# ── An unrestricted spawner withholds nothing ─────────────────────────

(with-temp-dir dir
               (let [target (path/join dir "allowed")]
                 (ev/join (ev/spawn (fn [] (file/write target "x"))))
                 (assert (path/exists? target)
                         "a fiber spawned outside any sandbox writes")))

(println "caps-spawn: OK")
