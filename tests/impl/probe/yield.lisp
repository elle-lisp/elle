(elle/epoch 15)
# audited: 2026-10-20
# Fiber-internal yielding loops, drained so loop-scope reclamation fires, and the channel send/receive round trip.
# docs/impl/region/diagnostics.md
# docs/impl/region/effects.md
#
# ── Fiber-internal yielding loops ─────────────────────────────────────
# The loop and the yield live inside the fiber. The run-block creates a fiber
# that runs b internal iterations then COMPLETES, and drains it — so loop-scope
# reclamation fires (a forever-generator never exits its loop, so per-iteration
# values that reclaim at scope-exit would falsely read as leaks). Per-iteration
# reclamation across the yield back-edge is what is under test.
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
(defn rate-yield [label body]
  (r:drive label (fn [b] (drain-block (yielding-fiber body) b))))
(println "── folded suite: fiber-internal yield ──")
(rate-yield "yield-struct" (fn [i] {:x i}))
(rate-yield "yield-string" (fn [i] (string "iter-" i)))
(rate-yield "yield-closure"
            (fn [i]
              (let [f (fn [] i)]
                (f))))
(rate-yield "yield-concat" (fn [i] (concat "x" (number->string i))))
(r:drive "yield-put"
         (fn [b]
           (drain-block (fn [n]
                          (when (%not (%int? n)) (error :n-not-int))
                          (fiber/new (fn []
                                       (def @st @{:data nil})
                                       (def @i 0)
                                       (while (%lt i n)
                                         (put st :data {:iter i})
                                         (yield i)
                                         (assign i (%add i 1)))) |:yield|)) b)))
(r:drive "yield-reassign"
         (fn [b]
           (drain-block (fn [n]
                          (when (%not (%int? n)) (error :n-not-int))
                          (fiber/new (fn []
                                       (def @v (string "init"))
                                       (def @i 0)
                                       (while (%lt i n)
                                         (assign v (string "val-" i))
                                         (yield i)
                                         (assign i (%add i 1)))) |:yield|)) b)))
(r:drive "yield-multimut"
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
                                         (assign i (%add i 1)))) |:yield|)) b)))
(r:drive "yield-spawn"
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
                                         (assign i (%add i 1)))) |:yield|)) b)))

# ── Channel send/recv — a genuinely Shared message ─────────────────────
# `chan/send` is the sole `RegionEffect::Sends` declarant: its message crosses the
# fiber frontier (it rides the channel buffer, by pointer, to the receiving fiber),
# so it can never be Owned by a bounded activation and stays on the per-region RC
# baseline (docs/impl/region/effects.md, docs/impl/region/ownership.md). The send
# seam increfs the message region at the enqueue to hold it in the buffer until
# received; the receive removes it from the buffer and lowers its region's count
# there. Reclaimed: rate 0. The fresh channel each block is created and freed
# within the run-block, so only the per-op message reclamation shows; it reads
# 2/op without the receive-side release.
(r:drive "chan-send-recv"
         (fn [b]
           (when (%not (%int? b)) (error :block-not-int))
           (let [[s r] (chan)]
             (def @i 0)
             (while (%lt i b)
               (chan/send s {:k i :v (string "v" i)})
               (chan/recv r)
               (assign i (%add i 1))))))
