(elle/epoch 18)
# audited: 2026-11-24
# A replica ahead of a tail call a `break` carries frees nothing the call or the caller still reads.
# docs/impl/region/breakpoint.md
#
# Soundness complement of region-break-tail-carry.lisp. The replica fires on
# the closure path where no release fired before, so it owes a count argument.
# Three ways it could be wrong, and each faults here under guardfree. It frees
# an operand the callee takes over. It frees a value the callee reaches through
# its captured environment. It frees a value the callee hands back to the
# caller.
#
# Every witness reads the subject's heap contents after the call, through a
# string inside a struct, so an early free faults rather than reading stale
# bytes. A fresh subject per iteration keeps region ids churning, so a freed
# region is recycled under the reader.

# ── callees, passed in so no graft inlines the call ─────────────────────────

(def @read-k (fn [s] (length s:k)))
(def @same (fn [s] s))
(def @call (fn [f] (f)))

# ── witnesses ────────────────────────────────────────────────────────────────

# (a) the callee takes the value bound outside the loop as its argument. The
# release behind the call is the ownership move, so no replica may go ahead.
(defn w-argument [g i]
  (let [held {:k (string "a" i "-long")}]
    (var n 0)
    (forever
      (when (>= n 2) (break (g held)))
      (assign n (+ n (length held:k))))))

# (b) the callee reaches the value through its captured environment. The
# replica drops the frame's reference, and the closure's own edge keeps it.
(defn w-capture [h i]
  (let [held {:k (string "c" i "-long")}
        f (fn [] (length held:k))]
    (var n 0)
    (forever
      (when (>= n 2) (break (h f)))
      (assign n (+ n (length held:k))))))

# (c) the callee hands the value back, and the caller reads it afterwards.
(defn w-handback-inner [g i]
  (let [held {:k (string "h" i "-long")}]
    (var n 0)
    (forever
      (when (>= n 2) (break (g held)))
      (assign n (+ n (length held:k))))))
(defn w-handback [g i]
  (let [r (w-handback-inner g i)]
    (length r:k)))

# (d) the value the loop makes on each pass is the call's argument on the
# breaking pass. Its jumped-over release is the ownership move there.
(defn w-jumped-argument [g i]
  (var n 0)
  (forever
    (let [x {:k (string "j" i "-" n)}]
      (when (>= n 2) (break (g x)))
      (assign n (+ n 1)))))

# (e) the break carries the joined chunks out to a caller that reads them.
(defn w-concat-inner [i]
  (let [chunks @[]]
    (var k 0)
    (forever
      (when (>= k 3) (break (apply concat chunks)))
      (push chunks (string "q" i "-" k))
      (assign k (inc k)))))
(defn w-concat [i]
  (length (w-concat-inner i)))

# ── control: the same read with no break, to check the harness ───────────────
(defn c-plain [i]
  (let [x {:k (string "p" i)}]
    (length x:k)))

# ── drive: a fresh subject each iteration; an early free faults on the read ──
(var i 0)
(var a 0)
(var b 0)
(var c 0)
(var d 0)
(var e 0)
(var k 0)
(while (< i 3000)
  (assign a (w-argument read-k i))
  (assign b (w-capture call i))
  (assign c (w-handback same i))
  (assign d (w-jumped-argument read-k i))
  (assign e (w-concat i))
  (assign k (c-plain i))
  (assign i (inc i)))

(assert (> k 0) "control: plain struct read mis-read (harness broken)")

(assert (> a 0) "an argument the callee takes over was freed ahead of the call")
(assert (> b 0) "a value the callee reaches through a capture was freed")
(assert (> c 0) "a value the callee hands back was freed under the caller")
(assert (> d 0) "a jumped-over value the call takes was freed ahead of it")
(assert (> e 0) "the joined chunks the break carried were freed")

(println "region-break-tail-carry-uaf: ok")
