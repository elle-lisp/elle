(elle/epoch 12)
# Channel send of an OWNED PARAMETER through a module-level channel — the shape
# where no compile-time region pair exists at the send site.
#
# `chan/send`'s message reference is counted at the send seam itself
# (`EscapeSite::ChanSend` in `prim_chan_send`, docs/impl/region/effects.md §
# `Sends`): the channel buffer is external to the region system, so the seam's
# runtime retain IS the reference that holds the message in the buffer until
# `release_received_message` lowers it at the receive. A compile-time edge cannot
# carry this reference here: the channel is a module-level binding read as an
# upvalue, so the solver has no region pair to key an incref on. Meanwhile the
# sending function's owned-parameter release is not gated on escape, so without
# the seam retain the caller's own releases drain the message's region to zero
# while the message still sits in the buffer — the receive then reads a freed
# region (a stale-region/wrong-tag deref, or unbounded recursion when `chan/recv`
# walks the freed buffer in tail position).
#
# Three shapes drive that seam, each with the message as an owned parameter of
# the sending function and the channel at module level:
#   1. a top-level caller loop (send + recv per iteration),
#   2. the same sender called from an `ev/spawn`'d fiber,
#   3. `chan/recv` in the sending function's tail.
# The value assertions are the correctness face (the received message is intact,
# not a freed-page read); `--trace=guardfree` over this file is the UAF gate
# (`region_chan_send_owned_param_uaf` in tests/integration/elle_scripts.rs). The
# bounded-growth gauge at the end is the leak face of the same seam: the send
# retain must be lowered by the receive, or every cycle strands one region.

(def [snd rcv] (chan))

(defn sendit [v]
  (chan/send snd v))

# ── Shape 1: top-level caller loop ────────────────────────────────────
(var i 0)
(var r nil)
(while (%lt i 200)
  (sendit (list (string "s" i) i))
  (assign r (chan/recv rcv))
  (assert (= (get r 0) :ok) "recv should observe the sent message")
  (let [msg (get r 1)]
    (assert (= (first msg) (string "s" i))
            "message head intact (not a freed-page read)")
    (assert (= (second msg) i) "message tail intact"))
  (assign i (%add i 1)))

# ── Shape 2: the same sender, called from a spawned fiber ─────────────
(var j 0)
(while (%lt j 200)
  (ev/join (ev/spawn (fn [] (sendit (list (string "t" j) j)))))
  (assign r (chan/recv rcv))
  (assert (= (get r 0) :ok) "recv should observe the fiber's message")
  (let [msg (get r 1)]
    (assert (= (first msg) (string "t" j)) "fiber-sent message head intact")
    (assert (= (second msg) j) "fiber-sent message tail intact"))
  (assign j (%add j 1)))

# ── Shape 3: chan/recv in the sending function's tail ─────────────────
# A freed buffer here manifested as unbounded recursion (stack overflow) rather
# than a bad read; a correct seam runs the loop flat.
(defn drive [v]
  (chan/send snd v)
  (chan/recv rcv))

(var k 0)
(while (%lt k 100)
  (let [got (drive (list 1 2 3))]
    (assert (= (get got 0) :ok) "tail recv should observe the sent message")
    (assert (= (first (get got 1)) 1) "tail-recv message intact"))
  (assign k (%add k 1)))

# ── Leak face: the seam retain must be lowered by the receive ─────────
(defn delta [thunk n]
  # One warmup run absorbs one-time setup regions, then measure the net
  # live-region growth the next n cycles add.
  (thunk 200)
  (let [before (arena/region-count)]
    (thunk n)
    (%sub (arena/region-count) before)))

(defn churn [n]
  (var m 0)
  (while (< m n)
    (sendit {:k m :v (string "v" m)})
    (let [got (chan/recv rcv)]
      (assert (= (get got 0) :ok) "churn recv should observe the message")
      (assert (= (get (get got 1) :k) m) "churn message key intact"))
    (assign m (%add m 1))))

# Discriminator: a genuine unbounded retain, so the gauge MUST climb ~1/op. If
# this does not grow, the gauge is dead and the churn verdict below is void.
(def @sink @[])
(defn grow [n]
  (var m 0)
  (while (< m n)
    (push sink {:k m})
    (assign m (%add m 1))))

(let [grow-slope (delta grow 2000)
      churn-slope (delta churn 2000)]
  (assert (%lt 1500 grow-slope)
          (string "gauge dead: discriminator grew only " grow-slope
                  " regions over 2000 ops — every reclaim verdict is void"))
  (assert (%lt churn-slope 200)
          (string "owned-param chan send/recv leaks the message region: growth "
                  churn-slope " over 2000 cycles (reclaimed ⇒ bounded, ≈0)")))

(println "region-chan-send-owned-param-uaf: ok")
