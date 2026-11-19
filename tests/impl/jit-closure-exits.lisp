(elle/epoch 15)
# audited: 2026-11-18
# A compiled function that holds a celled local parks a frame whose env holds the cell, so the resumed body and its closure share it.
# docs/impl/jit-closure.md
#
# Each subject counts in `acc`, which the closure `bump` captures and mutates,
# so `acc` lives in a cell. The compiled body holds that cell in a variable,
# and a side exit rebuilds the frame the interpreter would hold: the cell in
# the env slot of `acc`, nil in its stack slot. The interpreter that resumes
# the frame reads `acc` through the env and calls `bump`, which writes the
# same cell. Each subject leaves compiled code a different way: a yield, a
# call whose callee raises and is restarted, and a fuel pause.
#
# The counter-factual is a frame rebuilt as captures then params, with the
# cell left on the stack: the resumed read of `acc` finds no env slot, and the
# frame faults or answers a value `bump` never wrote. A JIT that refuses a
# function that builds a closure runs every subject interpreted, so the
# answers stay right and only `jit?` tells.
#
# The sidecar makes the JIT eager on the VM thread and turns the graft off.

(defn yields [n]
  (var acc n)
  (let [bump (fn [k] (assign acc (+ acc k)))]
    (bump 1)
    (let [r (yield acc)]
      (bump r)
      acc)))

(defn fail-with [x]
  (error {:error :boom :at x}))

(defn raises [n]
  (var acc n)
  (let [bump (fn [k] (assign acc (+ acc k)))]
    (bump 1)
    (let [r (fail-with acc)]
      (bump r)
      acc)))

# The branch that yields is never taken. It lets the function pause on fuel,
# which a silent function never does (docs/signals/inference.md).
(defn spins [n]
  (var acc 0)
  (let [bump (fn [] (assign acc (+ acc 1)))]
    (if (int? n)
      (do
        (var i 0)
        (while (< i n)
          (bump)
          (assign i (+ i 1)))
        acc)
      (yield n))))

# Warm each subject once on a fiber that catches what it raises, and wait for
# the compiles.
(fiber/resume (fiber/new (fn [] (yields 0)) |:yield|))
(fiber/resume (fiber/new (fn [] (raises 0)) |:error|))
(spins 1)
(jit/rejections)

(when (vm/config :jit)
  (assert (and (jit? yields) (jit? raises) (jit? spins))
          "the subjects compile, so each parks a compiled frame"))

# ── a yield ─────────────────────────────────────────────────────────────────
(let [f (fiber/new (fn [] (yields 10)) |:yield|)]
  (assert (= (fiber/resume f) 11) "the compiled body bumps acc before it yields")
  (assert (= (fiber/resume f 5) 16)
          "the resumed body and the closure share acc's cell"))

# ── a call whose callee raises, restarted ───────────────────────────────────
(let [f (fiber/new (fn [] (raises 10)) |:error|)]
  (assert (= (get (fiber/resume f) :at) 11) "the callee raises with acc read")
  (assert (= (fiber/resume f 5) 16)
          "the restarted body and the closure share acc's cell"))

# ── a fuel pause ────────────────────────────────────────────────────────────
(let [f (fiber/new (fn [] (spins 1000)) |:fuel|)]
  (var pauses 0)
  (fiber/set-fuel f 50)
  (fiber/resume f)
  (while (= (fiber/status f) :paused)
    (assign pauses (+ pauses 1))
    (fiber/set-fuel f 50)
    (fiber/resume f))
  (assert (> pauses 0) "the loop pauses on fuel")
  (assert (= (fiber/status f) :dead) "the refueled loop finishes")
  (assert (= (fiber/value f) 1000)
          "every pause resumes on the cell the closure bumps"))

(println "jit-closure-exits: ok")
