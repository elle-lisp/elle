(elle/epoch 12)
# tests/elle/region-capability-denial-value.lisp
#
# Counterfactual for a use-after-free in the capability-denial path
# (the posix.lisp test-6 / arena.rs:306 "tag/object mismatch" crash:
# value.tag=14 TAG_STRUCT pointing at recycled memory now holding an
# ffi-signature).
#
# When a fiber calls a primitive whose signal bits overlap its withheld
# capabilities, `handle_capability_denial` (src/vm/signal.rs) builds a
# `{:error :capability-denied ...}` payload struct and suspends the fiber
# with that struct in `fiber.signal` — read later via `fiber/value`.
#
# That payload escapes into `fiber.signal` exactly like a yielded value, but
# the denial handler — unlike the `SignalAction::Suspend` path — forgot to
# `incref_for_escape(.., SuspendEscape)` it. So the struct had rc=1 (its
# owning activation scope only); the resumer's `DecrefValueRegion` on the
# resume result dropped it to 0 and freed the region while `fiber.signal`
# still referenced it. `(fiber/value f)` then derefed freed memory.
#
# This crashed deterministically even with --jit=off; `make smoke`'s posix
# test hit it via :os-signal denial. Here we use :io denial so the test is
# independent of FFI / platform features and runs in every smoke mode.
#
# The fix: the denial handler retains the payload region with SuspendEscape,
# mirroring the yield/suspend path. (A never-resumed suspended fiber still
# leaks its payload region — the known suspended-frame-release gap — but the
# yield path leaks identically; that is a separate, non-corruption issue.)

# A denied call in Call position (the body's last form is NOT a tail call to
# the denied primitive: `do` keeps it mid-activation).
(defn denied-call []
  (let [f (fiber/new (fn []
                       (do
                         (println "should be blocked")
                         1)) |:error :io| :deny |:io|)]
    (fiber/resume f)
    (assert (= (fiber/status f) :paused) "fiber pauses after :io denial")  # Read several fields — each derefs the payload struct that the buggy
    # path had already freed.
    (let [val (fiber/value f)]
      (assert (= :capability-denied (get val :error))
              "payload :error survives resume")
      (assert ((get val :denied) :io)
              "payload :denied set survives resume and contains :io")
      (assert (= "port/write" (get val :primitive))
              "payload :primitive string survives resume")
      val)))

# Allocate heavily between denial and read across many iterations so a
# prematurely-freed payload region is likely to be recycled into a different
# HeapObject variant (the tag/object mismatch the original crash showed).
(each i (range 0 200)
  (let [v (denied-call)]
    (def junk (@string))
    (%string-push junk "xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx")
    (assert (= :capability-denied (get v :error))
            "denial payload still valid after intervening allocation")))

(println "region-capability-denial-value: OK")
