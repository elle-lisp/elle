(elle/epoch 13)
# audited: 2026-09-29
# A capability denial's payload stays readable through fiber/value after the resumer releases its result.
# docs/impl/region/park.md
#
# When a fiber calls a primitive whose signal bits overlap its withheld
# capabilities, `handle_capability_denial` (src/vm/signal.rs) builds a
# `{:error :capability-denied ...}` payload struct and suspends the fiber with
# that struct in `fiber.signal`, read later via `fiber/value`. The payload
# escapes into `fiber.signal` like a yielded value, so the handler retains its
# region with `SuspendEscape`, as the yield path does.
#
# The counter-factual: without that retain the struct has rc=1, the resumer's
# `DecrefValueRegion` on the resume result frees it while `fiber.signal` still
# names it, and `(fiber/value f)` derefs freed memory — a tag/object mismatch
# once the page is recycled. It fails the same way with the JIT off. An `:io`
# denial keeps the file independent of FFI and platform features.

# A denied call in Call position (the body's last form is NOT a tail call to
# the denied primitive: `do` keeps it mid-activation).
(defn denied-call []
  (let [f (fiber/new (fn []
                       (do
                         (println "should be blocked")
                         1)) |:error :io| :deny |:io|)]
    (fiber/resume f)
    (assert (= (fiber/status f) :paused) "fiber pauses after :io denial")
    # Read several fields — each derefs the payload struct that a missing
    # retain would already have freed.
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
