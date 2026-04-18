(elle/epoch 8)
## Fuel: Preemption via Instruction Budget
##
## Tests for fiber fuel/preemption (issue #585).
## Covers: backward-jump exhaustion, call exhaustion, forward-jump non-exhaustion,
## refuel-and-resume, unlimited fibers, zero fuel, signal masks, remaining-fuel
## reads, clear-fuel, and nested-fiber independence.

# ============================================================================
# Scenario 1: Backward jump exhausts fuel
# ============================================================================
#
# A tight counting loop. Each iteration performs one backward Jump.
# With fuel=5 the fiber completes at most 5 iterations then pauses.
# The fiber mask |:fuel| ensures the signal is caught by the fiber itself
# so we can observe :paused status directly without a wrapper.

(let [f (fiber/new
            (fn []
              (letrec [loop (fn (n)
                               (if (< n 100)
                                   (loop (+ n 1))
                                   n))]
                (loop 0)))
            |:fuel|)]
  (fiber/set-fuel f 5)
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "backward-jump: fiber paused on fuel exhaustion")
  (assert (= (fiber/value f) nil) "backward-jump: fuel signal carries nil payload"))

# ============================================================================
# Scenario 2: TailCall instruction exhausts fuel
# ============================================================================
#
# A recursive function where the recursive call is in tail position, compiling
# to TailCall. Each TailCall decrements fuel. With fuel=3 the fiber pauses
# after 3 tail calls.
#
# Note: (= n 0) compiles to the Eq opcode — primitives accessed as opcodes
# don't consume fuel. Only Call/TailCall instructions (calls to closures or
# native functions) consume fuel.

(let [f (fiber/new
            (fn []
              (defn count-calls [n]
                (if (= n 0)
                    "done"
                    (count-calls (- n 1))))
              (count-calls 100))
            |:fuel|)]
  (fiber/set-fuel f 3)
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "call-exhaustion: fiber paused"))

# ============================================================================
# Scenario 3: Forward jumps do NOT decrement fuel
# ============================================================================
#
# An if expression with fuel=1. The if compiles to JumpIfFalse (a conditional
# forward jump) and a forward Jump for the else branch — neither consumes fuel.
# With fuel=1 the fiber should complete without exhausting fuel.
#
# If this test fails with :paused instead of :dead, a forward Jump is
# incorrectly decrementing fuel.

(let [f (fiber/new
            (fn []
              (if true
                  42
                  0))
            |:fuel|)]
  (fiber/set-fuel f 1)
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "forward-jump: fiber completes with fuel=1")
  (assert (= (fiber/value f) 42) "forward-jump: correct return value"))

# ============================================================================
# Scenario 4: Refuel and resume
# ============================================================================
#
# Exhaust fuel mid-loop, refuel with enough to finish, resume, verify the
# fiber continues from where it paused and produces the correct final value.

(let [f (fiber/new
            (fn []
              (letrec [loop (fn (n acc)
                               (if (< n 10)
                                   (loop (+ n 1) (+ acc n))
                                   acc))]
                (loop 0 0)))
            |:fuel|)]
  (fiber/set-fuel f 3)
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "refuel: initially paused")
  (fiber/set-fuel f 1000)
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "refuel: fiber completes after refuel")
  (assert (= (fiber/value f) 45) "refuel: correct final value (0+1+...+9=45)"))

# ============================================================================
# Scenario 5: Unlimited fiber runs to completion
# ============================================================================
#
# A fiber without fuel set (mask=0, no fuel) runs a loop to completion.
# This verifies that the default unlimited path is unaffected.

(let [f (fiber/new
            (fn []
              (letrec [loop (fn (n acc)
                               (if (< n 100)
                                   (loop (+ n 1) (+ acc n))
                                   acc))]
                (loop 0 0)))
            0)]
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "unlimited: fiber completes")
  (assert (= (fiber/value f) 4950) "unlimited: correct sum (0+...+99=4950)"))

# ============================================================================
# Scenario 6: Zero fuel causes immediate signal
# ============================================================================
#
# Setting fuel to 0 means the very first fuel checkpoint fires immediately.
# An infinite loop with fuel=0 pauses before executing a single iteration.

(let [f (fiber/new
            (fn []
              (letrec [loop (fn (n) (loop (+ n 1)))]
                (loop 0)))
            |:fuel|)]
  (fiber/set-fuel f 0)
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "zero-fuel: pauses immediately"))

# ============================================================================
# Scenario 7: Fuel signal caught by mask
# ============================================================================
#
# When the fiber's mask includes :fuel, fiber/resume returns the signal's
# payload (nil) to the parent and the fiber becomes :paused.

(let [f (fiber/new
            (fn []
              (letrec [loop (fn () (loop))]
                (loop)))
            |:fuel|)]
  (fiber/set-fuel f 2)
  (let [result (fiber/resume f)]
    (assert (= (fiber/status f) :paused) "mask-catch: fiber is paused")
    (assert (= result nil) "mask-catch: caught signal delivers nil to parent")))

# ============================================================================
# Scenario 8: Fuel signal propagates when not in mask
# ============================================================================
#
# Without :fuel in the inner fiber's mask (mask=0), the signal propagates to
# the parent. The outer fiber has |:fuel| so the root catches it from there.
#
# Signal propagation mechanics:
#   inner.mask=0  → inner does NOT catch :fuel → propagates to outer
#   outer.mask=|:fuel| → outer's parent (root) DOES catch :fuel from outer
#   outer.status = :paused, inner.status = :paused

(let [inner (fiber/new
                (fn []
                  (letrec [loop (fn () (loop))]
                    (loop)))
                 0)]
  (fiber/set-fuel inner 2)
  (let [outer (fiber/new
                  (fn [] (fiber/resume inner))
                  |:fuel|)]
    (fiber/resume outer)
    (assert (= (fiber/status outer) :paused) "propagate: outer paused by propagated fuel signal")
    (assert (= (fiber/status inner) :paused) "propagate: inner also paused")))

# ============================================================================
# Scenario 9: fiber/fuel reads remaining budget
# ============================================================================
#
# Set fuel, read it back before and after exhaustion.
# After exhaustion, fiber/fuel returns 0 — the check fires when fuel==0
# (before decrement), so the stored value at exhaustion is 0.

(let [f (fiber/new
            (fn []
              (letrec [loop (fn (n) (loop (+ n 1)))]
                (loop 0)))
            |:fuel|)]
  (fiber/set-fuel f 10)
  (assert (= (fiber/fuel f) 10) "fuel-read: reads 10 before resume")
  (fiber/resume f)
  (assert (= (fiber/fuel f) 0) "fuel-read: reads 0 after exhaustion"))

# ============================================================================
# Scenario 10: fiber/clear-fuel removes budget
# ============================================================================
#
# Set fuel, exhaust it, clear it, then resume — the fiber should run to
# completion because clear-fuel restores unlimited execution.

(let [f (fiber/new
            (fn []
              (letrec [loop (fn (n acc)
                               (if (< n 50)
                                   (loop (+ n 1) (+ acc n))
                                   acc))]
                (loop 0 0)))
            |:fuel|)]
  (fiber/set-fuel f 5)
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "clear-fuel: paused initially")
  (fiber/clear-fuel f)
  (assert (= (fiber/fuel f) nil) "clear-fuel: fuel is nil after clear")
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "clear-fuel: runs to completion after clear"))

# ============================================================================
# Scenario 11: Nested fibers have independent fuel
# ============================================================================
#
# Inner and outer fibers each have their own fuel counter. Exhausting inner's
# fuel does not affect outer's fuel — outer continues and completes normally.

(let [inner (fiber/new
                (fn []
                  (letrec [loop (fn (n) (loop (+ n 1)))]
                    (loop 0)))
                |:fuel|)]
  (let [outer (fiber/new
                  (fn []
                    (fiber/set-fuel inner 3)
                    (fiber/resume inner)
                    # inner is now paused; outer continues unaffected
                    (assert (= (fiber/status inner) :paused) "nested: inner paused")
                    42)
                  |:fuel|)]
    (fiber/set-fuel outer 1000)
    (fiber/resume outer)
    (assert (= (fiber/status outer) :dead) "nested: outer completes independently")
    (assert (= (fiber/value outer) 42) "nested: outer return value correct")
    (assert (= (fiber/status inner) :paused) "nested: inner remains paused")))
