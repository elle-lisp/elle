(elle/epoch 13)
# audited: 2026-09-29
# Hard-killing fibers in a loop frees nothing a live frame still counts on.
# docs/impl/region/owner.md
#
# `fiber/cancel` of a parked or stopped fiber and `fiber/abort` of a
# not-yet-started one are hard kills: both route through `kill_fiber`
# (src/vm/fiber/owned.rs), which consumes the parked chain and frees everything
# the fiber owns. The node-freeing half is pinned Rust-side, where a test can
# read a member's generation (the `runtime::tests::ownership::fnode::kill`
# tests). This file pins the other side on the production path. The sidecar
# arms guardfree, which turns an over-release into a fault. The kill's region
# residue is gauged by the `cancel-discard` probe in
# tests/impl/probe/concurrent.lisp, not here.

(defn park-and-cancel []
  (let [f (fiber/new (fn []
                       (yield 1)
                       2) |:yield|)]
    (fiber/resume f nil)
    (assert (= (fiber/value f) 1) "the body parks at its first yield")
    (fiber/cancel f :killed)
    (assert (= (fiber/status f) :dead) "cancel hard-kills the parked fiber")
    (assert (= (fiber/value f) :killed) "the kill leaves a readable error value")))

# A fiber stopped on an error holds its payload as its result, and the kill
# replaces it. The trap: the payload is a heap value the caller keeps reading
# after the kill, so a kill that releases it twice frees it under the caller.
(defn raise-and-cancel []
  (let* [payload @[:boom]
         f (fiber/new (fn [] (list :got (+ 1 (error payload)))) |:error|)]
    (assert (identical? (fiber/resume f) payload) "the body stops on its raise")
    (fiber/cancel f [:killed])
    (assert (= (fiber/status f) :dead) "cancel hard-kills the stopped fiber")
    (assert (= (fiber/value f) [:killed]) "the kill leaves its own value")
    (assert (= (get payload 0) :boom) "the displaced payload is still readable")))

(defn abort-new []
  (let [f (fiber/new (fn [] 42) |:yield|)]
    (fiber/abort f :never-started)
    (assert (= (fiber/status f) :error)
            "abort of a :new fiber errors it without running it")))

(var n 0)
(while (< n 250)
  (park-and-cancel)
  (raise-and-cancel)
  (abort-new)
  (assign n (+ n 1)))

# Fresh fiber machinery must read intact state after 750 kills (an
# over-releasing teardown drains live regions, and this is where the stale
# read would surface).
(def coro (fiber/new (fn [] (yield 3)) |:yield|))
(fiber/resume coro nil)
(assert (= (fiber/value coro) 3) "post-kill: a fresh fiber yields intact")

(println "region-fiber-cancel: ok")
