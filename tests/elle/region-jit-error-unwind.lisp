(elle/epoch 12)
# A COMPILED frame's error exit runs the releases it still owes
# (docs/impl/region/mechanism.md § "An abandoned frame runs the releases it
# still owes").
#
# An error leaves through the signal machinery on either tier, so none of the
# frame's remaining instructions run and every release it still owed is among
# them. The walk that runs them belongs to the runtime, not to the interpreter:
# a compiled frame reads its value route off the locals it spills at the exit
# and its slot route off the activation map its prologue pushed.
#
# This file is the COMPILED face of that gauge. The interpreter face is
# tests/elle/region-error-unwind.lisp, which raises natively; every raise here is
# an `(error v)`, whose `Emit` mints the payload's delivery itself, so nothing
# in the raising frame is exempt and its own reference is owed too.
#
# One subject per compiled error exit:
#
#   - `raise-owned` leaves through the `Emit` of `SIG_ERROR` itself. It parks no
#     frame, so the activation is abandoned where it stands, holding the string
#     it built for the payload.
#   - `caller-holds` leaves through the check after the call that raised,
#     holding a binding whose last use lies past that call.
#
# Each subject is driven until `jit?` reports it compiled, so the measured
# window is the compiled tier's. Under `--jit=off` nothing compiles, `jit?`
# stays false, and the same window gauges the interpreter walk.

(def window 500)
# The compile is asynchronous under `--jit=eager`: the worker installs while the
# interpreter keeps running the function, so the drive must outlast the compile
# rather than assume it. The cap only bounds that wait, and a policy that
# compiles nothing would pay it in full for no coverage — so the warm-up is
# skipped outright when the JIT is off, and the window below still gauges the
# interpreter walk.
(def jit-live? (not (= (vm/config :jit) :off)))
(def warm-cap 20000)

# subjects ─────────────────────────────────────────────────────────────────────

(defn raise-owned [j]
  (error (string "x" j)))

# The raise is a STATEMENT here: a bare call in the try body would be a
# frame-replacing tail call, which lands in the parked body frame instead of
# leaving a callee frame to walk.
(defn drive-raise [j]
  (try
    (begin
      (raise-owned j)
      nil)
    (catch e nil)))

(defn caller-holds [j]
  (let [held (string "h" j)]
    (begin
      (raise-owned j)
      held)))

(defn drive-holds [j]
  (try
    (begin
      (caller-holds j)
      nil)
    (catch e nil)))

# control ──────────────────────────────────────────────────────────────────────

# The same allocation and the same binding with NO raise: the ordinary releases
# run. A subject that reads like this one is measuring the abandoned release and
# not its own scratch.
(defn holds-no-raise [j]
  (let [held (string "h" j)]
    (string/size-of held)))

(defn drive-none [j]
  (try
    (begin
      (holds-no-raise j)
      nil)
    (catch e nil)))

# measurement ──────────────────────────────────────────────────────────────────

(defn all-hot? []
  (and (jit? raise-owned) (jit? caller-holds) (jit? holds-no-raise)))

# One interleaved drive, not three waits: the first pass submits every subject
# and the worker installs them all while the loop keeps running, so the whole
# warm-up costs one compile latency instead of three.
(defn warm []
  (var i 0)
  (while (and jit-live? (%lt i warm-cap) (not (all-hot?)))
    (drive-raise i)
    (drive-holds i)
    (drive-none i)
    (assign i (%add i 1)))
  (all-hot?))

(defn measure [thunk window]
  (def before (arena/count))
  (var j 0)
  (while (%lt j window)
    (thunk j)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

(def hot (warm))
(def d-raise (measure drive-raise window))
(def d-holds (measure drive-holds window))
(def d-none (measure drive-none window))

(println "region-jit-error-unwind over " window
         " iters (object deltas, compiled " hot "):")
(println "  raise-owned  " d-raise)
(println "  caller-holds " d-holds)
(println "  no-raise     " d-none " (control)")

(assert (%lt d-none 50)
        (concat "control: the same values with no raise reclaim normally, "
                "delta=" (number->string d-none)))
(assert (%lt d-raise 50)
        (concat "the emit-raised payload's own region is a release the raising "
                "frame still owed, delta=" (number->string d-raise)))
(assert (%lt d-holds 50)
        (concat "a binding live across the raising call is owed a release by "
                "the frame the raise unwinds, delta=" (number->string d-holds)))
(println "region-jit-error-unwind: ok")
