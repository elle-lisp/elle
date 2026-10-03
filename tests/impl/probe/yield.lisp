(elle/epoch 12)
# audited: 2026-09-08
# Fiber-internal yielding loops, drained so loop-scope reclamation fires, and the channel send/receive round trip.
#
# docs/impl/region/diagnostics.md
# ── Fiber-internal yielding loops ─────────────────────────────────────
# The loop and the yield live inside the fiber. The run-block creates a fiber
# that runs b internal iterations then COMPLETES, and drains it — so loop-scope
# reclamation fires exactly as in the originals (a forever-generator never exits
# its loop, so per-iteration values that reclaim at scope-exit would falsely read
# as leaks). Flip rotation at the yield back-edge is the mechanism under test.
(defn drain-block [make b]
  (let [f (make b)]
    (while (not= (fiber/status f) :dead) (fiber/resume f))))
(defn yielding-fiber [body]
  "(fn [n]) → a fiber that runs (body i), yields, n times, then completes."
  (fn [n]
    (when (%not (%int? n)) (error :n-not-int))
    (fiber/new (fn []
                 (def @i 0)
                 (while (%lt i n)
                   (body i)
                   (yield i)
                   (assign i (%add i 1)))) |:yield|)))
(defn pin-yield [label body rate]
  (pin (measure-core label (fn [b] (drain-block (yielding-fiber body) b))
                     count-gauge 100 6 60 0.4 0.5) rate))
(println "── folded suite: fiber-internal yield ──")
(pin-yield "yield-struct" (fn [i] {:x i}) 0)
(pin-yield "yield-string" (fn [i] (string "iter-" i)) 0)
(pin-yield "yield-closure"
           (fn [i]
             (let [f (fn [] i)]
               (f))) 0)
(pin-yield "yield-concat" (fn [i] (concat "x" (number->string i))) 0)
(pin (measure-core "yield-put"
                   (fn [b]
                     (drain-block (fn [n]
                                    (when (%not (%int? n)) (error :n-not-int))
                                    (fiber/new (fn []
                                      (def @st @{:data nil})
                                      (def @i 0)
                                      (while (%lt i n)
                                        (put st :data {:iter i})
                                        (yield i)
                                        (assign i (%add i 1)))) |:yield|)) b))
                   count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "yield-reassign"
                   (fn [b]
                     (drain-block (fn [n]
                                    (when (%not (%int? n)) (error :n-not-int))
                                    (fiber/new (fn []
                                      (def @v (string "init"))
                                      (def @i 0)
                                      (while (%lt i n)
                                        (assign v (string "val-" i))
                                        (yield i)
                                        (assign i (%add i 1)))) |:yield|)) b))
                   count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "yield-multimut"
                   (fn [b]
                     (drain-block (fn [n]
                                    (when (%not (%int? n)) (error :n-not-int))
                                    (fiber/new (fn []
                                      (def @sess
                                        @{:count 0 :last nil :streams @{}})
                                      (def @i 0)
                                      (while (%lt i n)
                                        (let [frame {:type :data
                                          :stream-id i
                                          :payload (string "p-" i)}]
                                          # field reads are untyped; the
                                          # allocation-free guard proves the
                                          # %add operand
                                          (let [c sess:count]
                                            (when (%not (%int? c))
                                              (error :count-not-int))
                                            (put sess :count (%add c 1)))
                                          (put sess :last frame)
                                          (put sess:streams i frame))
                                        (yield i)
                                        (assign i (%add i 1)))) |:yield|)) b))
                   count-gauge 100 6 60 0.4 0.5) 0)
(pin (measure-core "yield-spawn"
                   (fn [b]
                     (drain-block (fn [n]
                                    (when (%not (%int? n)) (error :n-not-int))
                                    (fiber/new (fn []
                                      (def @i 0)
                                      (while (%lt i n)
                                        (let [label (string "task-" i)
                                          f (fiber/new (fn []
                                            (string label "-done")) |:yield|)]
                                          (fiber/resume f))
                                        (yield i)
                                        (assign i (%add i 1)))) |:yield|)) b))
                   count-gauge 100 6 60 0.4 0.5) 0)

# ── Channel send/recv — the genuinely-Shared (class 7) incoming-count ──
# `chan/send` is the sole `RegionEffect::Sends` declarant: its message crosses the
# fiber frontier (it rides the channel buffer, by pointer, to the receiving fiber),
# so it can never be Owned by a bounded activation and stays on the incoming-count
# (per-region RC) path — the always-Shared class. The send seam increfs the
# message region at the enqueue to hold it in the buffer until received ("a store
# into a Shared region bumps its count"); the receive removes it from the buffer, so
# its region's incoming count is lowered there ("an overwrite/drop lowers it" —
# region/ownership.md § class 7, the Shared incoming-count). Reclaimed: rate 0. The
# fresh channel each block is created and freed within the run-block, so only the
# per-op message reclamation shows; RED (2/op) without the receive-side release.
(pin (measure-core "chan-send-recv"
                   (fn [b]
                     (when (%not (%int? b)) (error :block-not-int))
                     (let [[s r] (chan)]
                       (def @i 0)
                       (while (%lt i b)
                         (chan/send s {:k i :v (string "v" i)})
                         (chan/recv r)
                         (assign i (%add i 1))))) count-gauge 100 6 60 0.4 0.5)
     0)
