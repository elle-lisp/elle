(elle/epoch 12)
# ── The terminal-fiber teardown, exercised in a loop ──
#
# `fiber/cancel` of a parked fiber and `fiber/abort` of a not-yet-started
# one are hard kills: both route through `kill_fiber` (src/vm/fiber.rs),
# which consumes the parked chain and frees everything the fiber owns —
# each parked frame's activation owner node and the fiber owner node
# (docs/impl/region/owner.md § "Owner nodes" — "Fiber teardown frees
# everything the fiber owns"). No production lowering emits owner-node
# adopts yet, so the node-freeing half is pinned Rust-side
# (`runtime::tests::ownership::fiber_kill_frees_parked_and_fiber_owned`);
# this file pins the OTHER side on the production path: killing parked and
# new fibers in a loop frees nothing a live frame still counts on
# (`--trace=guardfree`, full stdlib, is the oracle that turns an
# over-release into a fault). The kill's per-op region RESIDUE — the
# suspending resume's carrier retain, which only a completing resume
# releases — is tracked by `oracle.lisp`'s `cancel-discard` probe, not here.

(defn park-and-cancel []
  (let [f (fiber/new (fn []
                       (yield 1)
                       2) |:yield|)]
    (fiber/resume f nil)
    (assert (= (fiber/value f) 1) "the body parks at its first yield")
    (fiber/cancel f :killed)
    (assert (= (fiber/status f) :error) "cancel hard-kills the parked fiber")
    (assert (= (fiber/value f) :killed) "the kill leaves a readable error value")))

(defn abort-new []
  (let [f (fiber/new (fn [] 42) |:yield|)]
    (fiber/abort f :never-started)
    (assert (= (fiber/status f) :error)
            "abort of a :new fiber errors it without running it")))

(var n 0)
(while (< n 250)
  (park-and-cancel)
  (abort-new)
  (assign n (+ n 1)))

# Fresh fiber machinery must read intact state after 500 kills (an
# over-releasing teardown drains live regions, and this is where the stale
# read would surface).
(def coro (fiber/new (fn [] (yield 3)) |:yield|))
(fiber/resume coro nil)
(assert (= (fiber/value coro) 3) "post-kill: a fresh fiber yields intact")

(println "region-fiber-cancel: ok")
