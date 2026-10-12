(elle/epoch 15)
# audited: 2026-10-12
# Compiled code charges a fiber's fuel where the interpreter does, and pauses where the budget runs out.
# docs/impl/jit.md
#
# The sidecar makes the JIT eager. Each subject is warmed once and the worker
# drained with `jit/rejections`, so the fueled runs below enter compiled code
# whatever the timing. Each subject has a twin with the same calls and jumps,
# which the JIT refuses because it builds a closure, so the twin runs in the
# interpreter. Parity with the twin is the claim: one budget, the same pauses.
#
# The trap: a silent function spends fuel and never pauses
# (docs/signals/inference.md), so a pause has to come from a function that may
# signal. Every subject and every twin can yield, on a branch no run takes.
#
# The counter-factual: compiled code that charges nothing runs the issue's
# loop of a million trips on a budget of 1000 and returns :dead, and it spends
# one unit of fuel where its twin spends dozens.

(defn compile [f]
  (jit/rejections)
  (assert (jit? f) "the subject is compiled before its fueled run"))

(defn interpreted [f]
  (jit/rejections)
  (assert (not (jit? f)) "the twin stays in the interpreter"))

# ── The subjects and their twins ─────────────────────────────────────────

# A self tail call: the compiled loop charges once per trip.
(defn spin [i n]
  (if (%int? i)
    (if (%int? n)
      (if (%lt i n) (spin (%add i 1) n) i)
      (yield n))
    i))

(defn spin-twin [i n]
  (let [keep (fn [] i)]
    (if (%int? i)
      (if (%int? n)
        (if (%lt i n) (spin-twin (%add i 1) n) i)
        (yield keep))
      i)))

# A backward jump: `while` loops without a call.
(defn count-up [n]
  (if (%int? n)
    (do
      (var i 0)
      (while (%lt i n) (assign i (%add i 1)))
      i)
    (yield n)))

(defn count-up-twin [n]
  (let [keep (fn [] n)]
    (if (%int? n)
      (do
        (var i 0)
        (while (%lt i n) (assign i (%add i 1)))
        i)
      (yield keep))))

# A non-tail call in a loop: each trip calls `spin` and discards its value.
(defn drive [k n]
  (if (%int? k)
    (if (%lt 0 k)
      (do
        (spin 0 n)
        (drive (%sub k 1) n))
      k)
    (yield k)))

(defn drive-twin [k n]
  (let [keep (fn [] k)]
    (if (%int? k)
      (if (%lt 0 k)
        (do
          (spin-twin 0 n)
          (drive-twin (%sub k 1) n))
        k)
      (yield keep))))

# A spliced tail call.
(defn splice-spin [i n]
  (if (%int? i)
    (if (%int? n)
      (if (%lt i n)
        (splice-spin ;[(%add i 1) n])
        i)
      (yield n))
    i))

(defn splice-spin-twin [i n]
  (let [keep (fn [] i)]
    (if (%int? i)
      (if (%int? n)
        (if (%lt i n)
          (splice-spin-twin ;[(%add i 1) n])
          i)
        (yield keep))
      i)))

# A spliced non-tail call.
(defn splice-call [n]
  (if (%int? n)
    [(spin ;[0 n])]
    (yield n)))

(defn splice-call-twin [n]
  (let [keep (fn [] n)]
    (if (%int? n)
      [(spin-twin ;[0 n])]
      (yield keep))))

# A compiled caller over a compiled callee that pauses.
(defn outer [n]
  (if (%int? n) [(spin 0 n)] (yield n)))

(spin 0 3)
(count-up 3)
(drive 1 1)
(splice-spin 0 1)
(splice-call 1)
(outer 1)
(spin-twin 0 3)
(count-up-twin 3)
(drive-twin 1 1)
(splice-spin-twin 0 1)
(splice-call-twin 1)

(each f [spin count-up drive splice-spin splice-call outer]
  (compile f))
(each f [spin-twin count-up-twin drive-twin splice-spin-twin splice-call-twin]
  (interpreted f))

# ── Drivers ──────────────────────────────────────────────────────────────

# Run `body` on a fiber with a budget large enough to finish, and answer the
# fuel it spent with the value it returned.
(defn spent [body]
  (let [f (fiber/new body |:fuel|)]
    (fiber/set-fuel f 1000000)
    (fiber/resume f)
    (assert (= (fiber/status f) :dead) "spent: the run finishes on its budget")
    [(- 1000000 (fiber/fuel f)) (fiber/value f)]))

# Run `body` one unit of fuel at a time, and answer the pauses it took with
# the value it returned.
(defn steps [body]
  (let [f (fiber/new body |:fuel|)]
    (var n 0)
    (fiber/set-fuel f 1)
    (fiber/resume f)
    (while (= (fiber/status f) :paused)
      (assert (nil? (fiber/value f)) "steps: each pause carries a nil payload")
      (assign n (+ n 1))
      (fiber/set-fuel f 1)
      (fiber/resume f))
    (assert (= (fiber/status f) :dead) "steps: the stepped run finishes")
    [n (fiber/value f)]))

# ── S1: the issue's program pauses ───────────────────────────────────────

(let [f (fiber/new (fn [] [(spin 0 1000000)]) |:fuel|)]
  (fiber/set-fuel f 1000)
  (fiber/resume f)
  (assert (= (fiber/status f) :paused)
          "S1: a compiled self-tail-call loop pauses when its budget runs out")
  (assert (nil? (fiber/value f)) "S1: the pause carries a nil payload")
  (assert (= (fiber/fuel f) 0) "S1: the pause leaves the budget at 0")
  (fiber/set-fuel f 10000000)
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "S1: the refueled fiber finishes")
  (assert (= (fiber/value f) [1000000]) "S1: the loop resumes where it paused"))

# ── S2: a compiled backward jump pauses ──────────────────────────────────

(let [f (fiber/new (fn [] [(count-up 1000000)]) |:fuel|)]
  (fiber/set-fuel f 1000)
  (fiber/resume f)
  (assert (= (fiber/status f) :paused)
          "S2: a compiled while loop pauses when its budget runs out")
  (fiber/set-fuel f 10000000)
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "S2: the refueled fiber finishes")
  (assert (= (fiber/value f) [1000000]) "S2: the loop resumes where it paused"))

# ── S3: each charging instruction costs what it costs in the interpreter ──

(def pairs
  [["self tail call" (fn [] [(spin 0 10)]) (fn [] [(spin-twin 0 10)])]
   ["backward jump" (fn [] [(count-up 10)]) (fn [] [(count-up-twin 10)])]
   ["non-tail call" (fn [] [(drive 3 4)]) (fn [] [(drive-twin 3 4)])]
   ["spliced tail call" (fn [] [(splice-spin 0 10)])
    (fn [] [(splice-spin-twin 0 10)])]
   ["spliced call" (fn [] [(splice-call 10)]) (fn [] [(splice-call-twin 10)])]])

(each [name compiled twin] pairs
  (let [[cost value] (spent compiled)
        [twin-cost twin-value] (spent twin)]
    (assert (= value twin-value) (string "S3 " name ": both tiers agree"))
    (assert (= cost twin-cost)
            (string "S3 " name ": compiled code spent " cost
                    " units, the interpreter " twin-cost)))
  (let [[n value] (steps compiled)
        [twin-n twin-value] (steps twin)]
    (assert (= value twin-value)
            (string "S3 " name ": stepping changes nothing"))
    (assert (= n twin-n)
            (string "S3 " name ": compiled code paused " n
                    " times, the interpreter " twin-n))))

# ── S4: a compiled caller parks behind a compiled callee ─────────────────

(let [f (fiber/new (fn [] [(outer 1000)]) |:fuel|)]
  (fiber/set-fuel f 100)
  (fiber/resume f)
  (assert (= (fiber/status f) :paused)
          "S4: a pause in the callee pauses the caller")
  (fiber/set-fuel f 10000000)
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "S4: the refueled fiber finishes")
  (assert (= (fiber/value f) [[1000]])
          "S4: the caller receives its callee's value after the pause"))

# ── S5: an unmetered fiber runs compiled code to the end ─────────────────

(let [f (fiber/new (fn [] [(spin 0 1000000)]) |:fuel|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "S5: an unmetered fiber never pauses")
  (assert (= (fiber/value f) [1000000]) "S5: the loop runs to its end")
  (assert (nil? (fiber/fuel f)) "S5: an unmetered fiber still has no budget"))

(println "fuel-jit-pause: ok")
