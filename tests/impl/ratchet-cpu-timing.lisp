(elle/epoch 15)
# audited: 2026-10-13
# The ratchet's best-of times the calling thread's CPU, so a thread that waits adds nothing to a timing.
# lib/ratchet.md
# docs/testing.md
#
# The counter-factual: best-of over the wall clock reads the sleeping thunk at
# its full 50 ms or more, because a sleep never ends early. A performance gate
# timed that way reads every stretch its child spent waiting for a CPU, and
# fails on the work it exists to pass.

(def r ((import "std/ratchet")))

(def nap 0.05)

(defn sleeper []
  (time/sleep nap))

(defn spinner []
  (def @i 0)
  (while (%lt i 2000000) (assign i (%add i 1)))
  i)

(def [slept spun] (r:best-of 3 sleeper spinner))

(println "ratchet-cpu-timing: a " nap " s sleep read " slept " s, a spin read "
         spun " s")

(assert (< slept (/ nap 5))
        (concat "a thread that sleeps uses almost no CPU, yet the sleep read "
                (string slept) " s against the " (string nap) " s it waited"))
(assert (> spun slept)
        (concat "a thread that spins uses more CPU than one that sleeps, yet the spin read "
                (string spun) " s against the sleep's " (string slept) " s"))
