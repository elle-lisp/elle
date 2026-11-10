(elle/epoch 15)
# audited: 2026-10-21
# A call made for effect whose callee returns a fresh value from its tail releases that value.
#
# docs/impl/region/mechanism.md
#
# Two callees: `ret-first`, whose tail is the native pass-through `first`, and
# `ret-closure`, whose tail calls a closure that builds a struct. The caller
# discards each result as a statement. The callee hands its tail value to the
# caller, so the caller owes its release, and the object count stays flat.
#
# The sidecar turns the graft off. Under the graft each call becomes its
# callee's body (docs/impl/graft.md), no call is left, and the file would read
# nothing about a call's tail return.
#
# The gauge-live discriminator: `keep` pushes every struct it builds into a
# module-level sink, so the same loop over it must grow the object count. A flat
# reading beside a dead gauge would prove nothing.

(def window 2000)

(defn ret-first [xs]
  (first xs))

(defn mk [x]
  {:v x})

(defn ret-closure [x]
  (mk x))

(def @sink @[])
(defn keep [x]
  (push sink {:v x}))

(defn growth [thunk]
  "The object-count growth across WINDOW discarded calls of THUNK, after a warm-up."
  (var i 0)
  (while (%lt i 100)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (- (arena/count) before))

(def kept (growth (fn [] (keep 1))))
(assert (>= kept window)
        (string "the discriminator must grow the object count, got " kept))

(def passthrough (growth (fn [] (ret-first (list {:k 1})))))
(assert (< passthrough 50)
        (string "a discarded native pass-through tail leaked " passthrough
                " objects over " window " calls"))

(def closure (growth (fn [] (ret-closure 1))))
(assert (< closure 50)
        (string "a discarded closure tail leaked " closure " objects over "
                window " calls"))

(println "region-discarded-tail-return: ok")
