(elle/epoch 18)
# audited: 2026-11-21
# Denying :inspect closes what a closure captured, and leaves a call of the closure open.
# docs/signals/capabilities.md
#
# Before :inspect, fn/captures declared no bit but :error. A fiber that
# withheld every effect capability read the scheduler's :spawn function off a
# parameter, took the queue of runnable fibers out of its captures, and pushed
# to it. The counter-factual is a fn/captures that declares no :inspect: its
# call runs, and the fiber finishes where it should have parked.

(include-file "caps/spawn.lisp")

# Every bit a sandbox can withhold and still run code. A fiber denied :error
# calls almost no primitive, so its denial of fn/captures shows nothing.
(def every-effect
  |:yield :debug :ffi :halt :io :exec :gpu :os-signal :fs :net :ambient :import
   :inspect|)

(defn inspect-denial [deny body]
  "The denial body parks on, run in a fiber denied `deny`, or nil when it runs
   to its end."
  (let [f (fiber/new body |:inspect :error| :deny deny)]
    (fiber/resume f)
    (when (= (fiber/status f) :paused) (fiber/value f))))

(defn assert-denied [deny body what]
  "Assert that body, denied `deny`, parks on the denial of fn/captures."
  (let [d (inspect-denial deny body)]
    (assert (= (get d :error) :capability-denied) (string what ": is denied"))
    (assert (= (get d :primitive) "fn/captures")
            (string what ": the denial names fn/captures"))
    (assert (= (get d :denied) |:inspect|) (string what ": is denied :inspect"))))

(def counter
  (let [items @[1 2 3]]
    (fn [] (length items))))

# ── The bit ───────────────────────────────────────────────────────────

(assert (= (get (signals) :inspect) 21) "the registry names the inspect bit")
(assert ((fiber/caps) :inspect) "the root fiber holds :inspect")
(let [f (fiber/new (fn [] 42) |:error| :deny |:inspect|)]
  (assert (not ((fiber/caps f) :inspect))
          "fiber/caps omits :inspect when denied")
  (assert ((fiber/caps f) :io) ":deny |:inspect| leaves :io held"))

# ── A denied fiber reads no capture ───────────────────────────────────

(assert-denied |:inspect| (fn [] (fn/captures counter)) "fn/captures")
(assert-denied |:inspect| (fn [] (captures counter)) "its alias")
(assert-denied |:inspect| (fn [] (fn/captures 42)) "a value with no captures")

# The denied call never ran, so the array holds what it held.
(let [f (fiber/new (fn []
                     (each slot in (fn/captures counter)
                       (push (get slot :value) :planted))) |:inspect :error|
                   :deny |:inspect|)]
  (fiber/resume f)
  (assert (= (counter) 3) "a denied fiber changes nothing the closure holds"))

# ── The scheduler's queues stay closed ────────────────────────────────

# A fiber reaches the scheduler's functions through a parameter, and their
# captures are its queues.
(assert-denied every-effect (fn [] (fn/captures (get (*scheduler*) :spawn)))
               "the scheduler's spawn function")
(assert-denied every-effect (fn [] (fn/captures (get (*scheduler*) :step)))
               "the scheduler's step function")

# ── A call of the closure runs ────────────────────────────────────────

(let [f (fiber/new (fn [] (counter)) |:error| :deny |:inspect|)]
  (assert (= (fiber/resume f) 3) "a denied fiber calls a closure it was handed")
  (assert (= (fiber/status f) :dead) "and finishes"))

(let [f (fiber/new (fn [] [(fn/arity counter) (closure? counter)]) |:error|
                   :deny |:inspect|)]
  (assert (= (fiber/resume f) [0 true])
          "the predicates that read a closure's code stay open"))

# ── A fiber that holds the bit reads the capture ──────────────────────

(let [slots (fn/captures counter)]
  (assert (= 1 (length slots)) "counter captures its array")
  (assert (= 3 (length (get (first slots) :value))) "and the slot holds it"))

# ── A fiber a sandbox spawns keeps the denial ─────────────────────────

# The scheduler refuses the denial at the spawned fiber's own call site. The
# counter-factual: an ev/spawn mask that does not name :inspect, so the denial
# passes the scheduler and fails the file.
(assert (refused? (spawned |:inspect| (fn [] (protect (fn/captures counter))))
                  "fn/captures" :inspect)
        "a spawned fiber is refused its read of a capture")

# A process system started in the sandbox: its scheduler refuses the denial,
# so the process fiber's mask must name :inspect. The sandbox catches
# :inspect, so a denial that passed the process scheduler stops it there.
(def process ((import "std/process")))
(let [out @[]
      sandbox (fiber/new (fn []
                           (process:start
                             (fn [] (push out (protect (fn/captures counter))))))
                         |:error :inspect| :deny |:inspect|)]
  (fiber/resume sandbox)
  (assert (= (fiber/status sandbox) :dead)
          "the sandboxed process system finishes")
  (assert (refused? (get out 0) "fn/captures" :inspect)
          "a process is refused its read of a capture"))

# ── A macro body and a module's form withhold the bit ─────────────────

(let [[ok? err] (protect (eval '(begin
                                  (defmacro peer []
                                    (fn/captures (fn [] 1)))
                                  (peer))))]
  (assert (not ok?) "a macro body that reads a capture fails the compile")
  (assert (string/contains? (get err :message) "macro 'peer'")
          "the error names the macro")
  (assert (string/contains? (get err :message) "fn/captures")
          "and the primitive"))

(let [[ok? err] (protect (import/load-syntax '(fn/captures println)))]
  (assert (not ok?) "a module's form that reads a capture does not load")
  (assert (= :module-error (get err :error)) "the loader refuses it")
  (assert (string/contains? (get err :message) "fn/captures")
          "naming the primitive")
  (assert (string/contains? (get err :message) ":inspect") "and the bit"))

# ── The purity walk spends the bit ────────────────────────────────────

# The walk reads a closure through fn/captures, so a fiber denied :inspect
# cannot load a module whose value holds a closure. A value that holds none
# needs no read, and loads.
(assert-denied |:inspect| (fn [] (import/load-syntax '(fn [] 1)))
               "a load of a closure")
(let [f (fiber/new (fn [] (import/load-syntax '{:a [1 2]})) |:error|
                   :deny |:inspect|)]
  (assert (= (fiber/resume f) {:a [1 2]}) "a value with no closure loads"))

# ── The bit is in the signal a call infers ────────────────────────────

(defn compiles? [src]
  "Whether src compiles. Nothing in it runs, so a refusal is the compiler's."
  (first (protect (compile/whole-module src "<inspect>"))))

(let [sig (compile/signal (compile/analyze "(defn f [g] (fn/captures g))") :f)]
  (assert (contains? (get sig :bits) :inspect)
          "a function that reads a capture carries :inspect")
  (assert (not (get sig :silent)) "so it is not silent"))

(assert (compiles? "(defn f [g] (fn/captures g))") "the body alone compiles")
(assert (not (compiles? "(defn f [g] (silent!) (fn/captures g))"))
        "silent! refuses a body that reads a capture")

# ── What a reader of the primitive learns ─────────────────────────────

(assert (contains? (get (vm/primitive-meta "fn/captures") :bits) :inspect)
        "vm/primitive-meta names :inspect among the bits fn/captures declares")
(assert (not (contains? (get (vm/primitive-meta "fn/arity") :bits) :inspect))
        "and not among those fn/arity declares")
(assert (string/contains? (doc fn/captures) ":inspect")
        "the docstring names the bit an expansion withholds")

(println "caps-inspect: OK")
