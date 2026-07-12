(elle/epoch 12)
# Counterfactual: a BORROWED value behind an `(or borrowed fresh)` phi, passed as a
# TAIL-CALL ARGUMENT to an owned-param callee, is over-released — use-after-free.
#
# This is region-tail-move-borrow-uaf.lisp's exact class with the borrowed operand
# hidden behind a branch/phi. The tail-call arg retain (lower_call, the borrowed
# branch) hands an owned reference to the callee ONLY when the arg is recognized as
# borrowed. `tail_arg_is_borrowed` recognizes a bare `Var`/`DerefCell(Var)` upvalue
# — but an `(or s @{})` is an `Or` node, so a naive predicate returns false, the
# borrowed short-circuit operand `s` is pure-moved, and the owned-param callee's
# release drains the capture RC to a premature free:
#
#   - `s` is a by-value capture (closure env owns the capture-incref). At runtime
#     `(or s @{})` short-circuits to `s`, so the fresh `@{}` never allocates and the
#     value handed to the callee IS the borrowed capture.
#   - Pure-moving it (no retain) into an owned-param callee, which releases it,
#     decrements the env's capture reference with no matching incref. After enough
#     calls the region frees while the env STILL references it; the next capture read
#     touches a freed page: `stale region deref` panic (plain) or SIGSEGV (guardfree).
#
# The predicate must see THROUGH the phi: an `or`/`and`/`if`/`cond`/`match` argument
# is borrowed iff any of its value-producing leaves is a borrowed upvalue. The retain
# and the operand releases are value-gated (`IncrefValueRegion`/`DecrefValueRegion`
# resolve the runtime value's region), so a single retain balances BOTH arms — the
# borrow arm (retain matches the callee's release) and the fresh arm (retain +
# fresh-operand's own release both fire on the fresh region). Subjects B and C guard
# that balance from below (no over-free) and above (no over-incref leak).
#
# The robust oracle is `--trace=guardfree`. RED before the phi-aware predicate;
# GREEN after.

# ── subject A: the minimal witness (borrow arm always taken) ──────────
# `sink` ignores its arg and returns an immediate, so the only region that can be
# freed is the MOVED arg's. `v` is an owned param with no later use — released
# value-based at the tail call, the release that over-frees a borrowed arg.
(defn sink (v)
  0)

# `s` is captured BY VALUE into two closures. `tick` tail-moves `(or s @{})` into
# `sink`; `peek` reads a field. The capture is the struct's region's only keeper —
# a correct convention must not let `tick` free it.
(defn make-holder ()
  (let [s (struct :n 7)]
    (struct :tick (fn () (sink (or s @{}))) :peek (fn () (get s :n)))))

(def holder (make-holder))
(def tick (get holder :tick))
(def peek (get holder :peek))

(var i 0)
(while (%lt i 1000)
  (tick)
  (assign i (%add i 1)))

# RED: this read faults (the capture's region was freed under the env). GREEN: 7.
(assert (= (peek) 7)
        "an (or capture fresh) phi tail-moved into an owned-param callee was over-released")

# ── subject B: the borrowed arg ESCAPES via a mutable store (balance from below) ──
# The retain must BALANCE, not over-count, when the callee stores the arg: caller
# retain (+1) + store-escape incref (+1) - callee param release (-1) = +1, exactly
# the container's one legitimate reference. `bag` ends holding 500 refs to the ONE
# captured struct (real container growth), and the distinct-region delta stays ~0.
(def bag @[])
(defn stash (v)
  (push bag v))

(def stasher
  ((fn ()
     (let [s (struct :n 9)]
       (fn () (stash (or s @{})))))))

(def before-reg (arena/region-count))
(var j 0)
(while (%lt j 500)
  (stasher)
  (assign j (%add j 1)))

(assert (%lt (%sub (arena/region-count) before-reg) 50)
        "an (or borrowed fresh) arg escaping via a mutable store was over-incref'd (leak)")
(assert (= (length bag) 500) "the mutable store did not retain every reference")
(assert (= (get (get bag 0) :n) 9)
        "the stored captured struct was freed under the container (UAF)")

# ── subject C: the FRESH arm is actually taken (balance from above) ──────
# When the borrow operand is nil the phi selects the FRESH `@{}`, which the callee
# consumes. A phi-aware retain must not over-incref the fresh arm — the retain and
# the fresh operand's own release both land on the fresh region and cancel, so the
# consumed fresh struct frees each call and the distinct-region count stays bounded.
(defn take-fresh (flag)
  (let [s (if flag (struct :n 1) nil)]
    (sink (or s @{}))))

(def before-fresh (arena/region-count))
(var k 0)
(while (%lt k 500)
  (take-fresh false)
  (assign k (%add k 1)))

(assert (%lt (%sub (arena/region-count) before-fresh) 50)
        "the FRESH arm of an (or nil fresh) tail-moved arg leaked (retain not balanced by its own release)")

# ── subject D: the faithful protect + or shape (the real-world form) ──────
# `(protect (te (or state @{})))` wraps the body in a fiber closure that CAPTURES
# `state`; the fiber teardown cascade plus the callee's consume double-release the
# captured struct. Reads back the mutation across repeated calls.
(defn te (state)
  (put state :x 42)
  nil)
(defn execute (state)
  (protect (te (or state @{}))))
(def shared @{})
(var m 0)
(while (%lt m 200)
  (execute shared)
  (assign m (%add m 1)))
(assert (= (get shared :x) 42)
        "a struct passed through (protect (te (or state @{}))) was over-released")

(println "region-or-tail-move-borrow-uaf: ok")
