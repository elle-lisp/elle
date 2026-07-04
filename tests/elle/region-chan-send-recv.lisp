(elle/epoch 12)
# Channel send/recv — the genuinely-Shared (class 7) incoming-count shape.
#
# `chan/send` is the sole `RegionEffect::Sends` declarant: its message crosses the
# fiber frontier (it rides the channel buffer, by pointer, to whatever fiber
# receives it), so the message can never be Owned by a bounded activation and stays
# on the incoming-count (per-region RC) path — the always-Shared class
# (docs/impl/region-model.md § "Why this is hybrid"). The `Sends` edge increfs the
# message's region at the send site to keep it alive in the channel buffer until
# received (docs/impl/region-effects.md § `Sends`) — "a store into a Shared region
# bumps its count". Receiving the message removes it from the buffer, so its region's
# incoming count must be lowered — "an overwrite/drop lowers it"
# (region-model.md § class 7, the Shared incoming-count refinement). `chan/recv`,
# `chan/try-select`, and `chan/wait-ready`'s fast path each carry that release.
#
# Without the receive-side release the message region leaks one per send/recv cycle —
# unbounded RSS for a long-running producer/consumer loop. This asserts the cycle
# reclaims: region growth over a large send/recv loop is bounded, measured beside a
# known-live-growth discriminator so a dead/stubbed gauge cannot paint it green.
# The correctness half (the received value is intact, not a freed-page read) is the
# assertions on the recv results, and `--trace=guardfree` over this file is the UAF
# gate (a premature release would fault there, not here).

(defn delta [thunk n]
  # One warmup run absorbs the one-time channel/setup regions, then measure the
  # net live-region growth the next n cycles add.
  (thunk 200)
  (let [before (arena/region-count)]
    (thunk n)
    (%sub (arena/region-count) before)))

# The reclamation target: a fresh heap message (struct carrying a nested string —
# two regions) sent and received each iteration. The recv result is checked and
# discarded, so nothing but the send/recv accounting keeps the message alive.
(defn churn [n]
  (let [[s r] (chan)]
    (var i 0)
    (while (%lt i n)
      (chan/send s {:k i :v (string "v" i)})
      (let [got (chan/recv r)]
        (assert (= (get got 0) :ok) "recv should observe the sent message")
        (assert (= (get (get got 1) :k) i) "recv message key intact (not freed)"))
      (assign i (%add i 1)))))

# Discriminator: a genuine unbounded retain (every op keeps a fresh struct forever),
# so the gauge MUST climb ~1/op. If this does not grow, the gauge is dead and the
# churn verdict below would be a false green.
(def @sink @[])
(defn grow [n]
  (var i 0)
  (while (%lt i n)
    (push sink {:k i})
    (assign i (%add i 1))))

(let [grow-slope (delta grow 2000)
      churn-slope (delta churn 2000)]
  (assert (%lt 1500 grow-slope)
          (string "gauge dead: discriminator grew only " grow-slope
                  " regions over 2000 ops — every reclaim verdict is void"))
  # A reclaimed cycle is bounded: a handful of transient regions, never ~2/op×2000.
  (assert (%lt churn-slope 200)
          (string "chan send/recv leaks the message region: growth " churn-slope
                  " over 2000 send/recv cycles (reclaimed ⇒ bounded, ≈0)")))

(println "region-chan-send-recv: ok")
