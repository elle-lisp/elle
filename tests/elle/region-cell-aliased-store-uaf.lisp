(elle/epoch 12)
# audited: 2026-09-21
# Soundness complement of region-cell-aliased-store.lisp: the counted store's
# pin never runs ahead of a second name's read
# (docs/impl/region/bindings.md § "An aliased stored value takes the counted
# store"). The pin rule is a maximum over the alias's own binding-chain
# extension, so every read below must see a live value however the arms
# interleave — armed under --trace=guardfree, where a pin that lands early is a
# deterministic fault at the read.
#
# Each face runs past a priming loop so region ids recycle: a released page
# that is read again then carries a different generation and detonates.

# ── 1. The alias read AFTER the store, in and past its iteration ────────
(defn sum-and-first [n]
  (let [s [[1] [2] [3] [4]]]
    (let [@u nil]
      (def @acc 0)
      (def @i 0)
      (while (< i n)
        (let [x (get s i)]
          (when (nil? u) (assign u x))
          (assign acc (+ acc (get x 0))))
        (assign i (%add i 1)))
      (+ acc (get u 0)))))

(def @r1 0)
(while (< r1 300)
  (assert (= (sum-and-first 4) 11)
          "the alias and the container both read live values")
  (assign r1 (%add r1 1)))

# ── 2. The aliased forwarding chain — keep outlives both loops ──────────
# `keep` names the first loop's stored value; its read at the tail is the last
# claim on that region, and the pin must sit after it while the second loop's
# overwrites release the cell's own references.
(defn chain-keep [n]
  (let [@last (array 0 0)]
    (var i 0)
    (while (< i n)
      (assign last (array i 7))
      (assign i (%add i 1)))
    (var keep last)
    (var j 0)
    (while (< j n)
      (assign last (array j 9))
      (assign j (%add j 1)))
    (+ (get keep 1) (get last 1))))

(def @r2 0)
(while (< r2 300)
  (assert (= (chain-keep 3) 16)
          "the aliased link's value outlives the second loop's overwrites")
  (assign r2 (%add r2 1)))

# ── 3. The phi-carried returned value — the caller reads what the callee
#       conditionally stored ─────────────────────────────────────────────
(defn pick [c]
  (let [@x (%pair 1 2)]
    (if c (assign x (%pair 3 4)) nil)
    x))

(def @r3 0)
(while (< r3 300)
  (assert (= (first (pick true)) 3) "the stored pair survives the return")
  (assert (= (first (pick false)) 1) "the init survives the untaken arm")
  (assign r3 (%add r3 1)))

# ── 4. The content drop leaves every arm — and frees nothing early ──────
# One arm stores inside a loop, the other stores once; whichever ran, the
# container's final content must still be readable at the tail.
(defn either-arm [t n]
  (let [@u nil]
    (def @i 0)
    (if t
      (while (< i n)
        (let [x (%pair i i)]
          (assign u x))
        (assign i (%add i 1)))
      (assign u (%pair 9 9)))
    (first u)))

(def @r4 0)
(while (< r4 300)
  (assert (= (either-arm true 3) 2) "the storing arm's last value is live")
  (assert (= (either-arm false 3) 9) "the single-store arm's value is live")
  (assign r4 (%add r4 1)))

(println "region-cell-aliased-store-uaf: ok")
