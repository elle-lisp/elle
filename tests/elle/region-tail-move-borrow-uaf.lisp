(elle/epoch 12)
# Counterfactual: a BORROWED value (a closure's by-value capture) passed as a
# TAIL-CALL ARGUMENT to an owned-param callee is over-released — use-after-free.
#
# The owned-params calling convention (docs/impl/region-rules.md Rule 5) makes a
# tail-call arg a pure MOVE: the caller emits no `CallArgument` incref, and the
# owned-param callee releases the arg at its last use. That is sound ONLY when the
# caller actually OWNS a transferable reference — a value built in the body, an
# owned local, an owned param. It is UNSOUND for a BORROWED reference, because the
# caller has no owning ref to move:
#
#   - A value captured by-value into a closure env is owned by the env's
#     capture-incref (`alloc_obj` scans the env and increfs the value's region).
#     A closure body that reads that capture and passes it as a tail-call arg is
#     handing over a reference it does not own.
#   - Pure-moving it (no incref) into an owned-param callee, which then releases
#     it, decrements the env's capture reference. Each call drops the region's RC
#     by one with no matching incref; after enough calls the region frees while
#     the closure env STILL references it.
#   - The next read of the capture (`UpdateCapture` / a field access) touches the
#     freed page: `tag/object mismatch` panic (no guardfree) or SIGSEGV
#     (guardfree). docs/impl/region-rules.md Rules 5 (every escape increfs)
#     and 8 (no UAF).
#
# This is the minimal, scheduler-free, squelch-free shape of the
# `<stdlib>:1759` async-scheduler UAF: the scheduler's long-lived `pending` /
# `runnable` / `fiber-io` @structs are captured by-value by its nested closures
# and forwarded into `put`/`del`/`complete-fiber` tail calls, which release them
# as owned params — draining the capture refs until a live @struct's region frees.
# The squelch/attune fiber-resume path still carries an open residual of this
# family (region-squelch-fiber-uaf.lisp, RED under guardfree).
#
# GREEN since the borrowed tail-call arg retain landed: a borrowed arg hands the
# callee a fresh owning reference (incref) instead of being pure-moved. The robust
# oracle is `--trace=guardfree`.

# ── subjects ──────────────────────────────────────────────────────
# `sink` ignores its arg and returns an immediate, so the only thing that can be
# freed is the MOVED arg's region. `v` is an owned param: with no later use, the
# callee releases it value-based (the release that over-frees the borrowed arg).
(defn sink (v)
  0)

# `make-holder` builds a heap struct and captures it BY VALUE into two closures:
# `tick` tail-moves the captured struct into `sink`; `peek` reads a field. The
# struct's region is kept alive solely by these two captures — a correct
# convention must not let `tick` free it.
(defn make-holder ()
  (let [s (struct :n 7)]
    (struct :tick (fn () (sink s)) :peek (fn () (get s :n)))))

(def holder (make-holder))
(def tick (get holder :tick))
(def peek (get holder :peek))

# ── witness ───────────────────────────────────────────────────────
# Each (tick) tail-moves the borrowed capture into sink, which releases it. The
# struct's region RC is only as large as its capture count, so it underflows to a
# free within the first few iterations; subsequent ticks (and the final peek)
# read a freed @struct.
(var i 0)
(while (%lt i 1000)
  (tick)
  (assign i (%add i 1)))

# The capture must still be live and correct after 1000 tail-moves. RED: this read
# faults (the struct's region was freed under the env). GREEN: returns 7.
(assert (= (peek) 7)
        "a by-value capture tail-moved into an owned-param callee was over-released")

# ── subject (b): the borrowed arg ESCAPES the callee via a mutable store ──
# Guards the dual hazard of the fix: the per-arg borrowed-incref must BALANCE,
# not over-count, when the callee stores the arg into a mutable container (the
# value escapes via mutable-store RC, not just the param release). Accounting:
# caller incref (+1) + store escape incref (+1) - callee param release (-1) =
# +1, exactly the container's legitimate reference. So the captured struct's
# region neither leaks (over-incref) nor double-frees, and `bag` ends up holding
# 500 refs to the ONE captured struct — a real container growth, not a leak of
# distinct regions (region delta stays 0).
(def bag @[])
(defn stash (v)
  (push bag v))

(def stasher
  ((fn ()
     (let [s (struct :n 9)]
       (fn () (stash s))))))

(def before-reg (arena/region-count))
(var j 0)
(while (%lt j 500)
  (stasher)
  (assign j (%add j 1)))

(assert (%lt (%sub (arena/region-count) before-reg) 50)
        "a borrowed arg escaping via a mutable store was over-incref'd (leak)")
(assert (= (length bag) 500) "the mutable store did not retain every reference")
(assert (= (get (get bag 0) :n) 9)
        "the stored captured struct was freed under the container (UAF)")

(println "region-tail-move-borrow-uaf: ok")
