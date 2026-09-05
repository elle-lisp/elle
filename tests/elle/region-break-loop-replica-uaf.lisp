(elle/epoch 12)
# audited: 2026-09-05
# Soundness complement of region-break-loop-replica.lisp: the release replicated
# at a `break` must not free anything the breaking path still reads.
#
# The replica fires where no release fired before, so it owes a count argument
# (docs/impl/region/replicate.md). Three ways it could be wrong, and all of them
# fault here: it frees the value the break CARRIES, which the block is about to
# hand its consumer; it frees a value a longer-lived holder still names, which
# the frame-held admission is there to refuse; or it runs a second time on a path
# that already released, which the nil stamp is there to absorb.
#
# Every witness reads the subject's HEAP contents after the loop, through a chain
# long enough that an over-early free faults rather than reading stale but still
# mapped bytes. A fresh subject per iteration keeps region ids churning, so a
# freed region is recycled under the reader.

(def @sink @[])

# ── witnesses ────────────────────────────────────────────────────────────────

# (a) the break CARRIES the loop's own value out. Its release is pinned where the
# block's value is consumed, so the replica must leave it alone.
(defn w-carried (i)
  (let [r (forever
            (let [msg {:k (string "c" i) :n i}]
              (cond
                (= msg:n -1) (break nil)
                (= msg:k :never) (break nil)
                true (break msg))))]
    (length r:k)))

# (b) the break carries a BORROW out of the value — the element lives inside the
# region the replica would free, so the uncounted-read extension has to carry the
# whole value past the exit.
(defn w-carried-borrow (i)
  (let [r (forever
            (let [msg {:k (string "b" i "-long") :n i}]
              (cond
                (= msg:n -1) (break nil)
                (= msg:k :never) (break nil)
                true (break msg:k))))]
    (length r)))

# (c) the value ESCAPES into a container that outlives the frame before the
# break, and is read back out afterwards. The store's incref is what the replica
# must not take below zero.
(defn w-store (i)
  (forever
    (let [msg {:k (string "s" i) :n i}]
      (cond
        (= msg:n -1) (break nil)
        (= msg:k :never) (break nil)
        true (begin
               (push sink msg)
               (break nil)))))
  (length (get (get sink (%sub (length sink) 1)) :k)))

# (d) a closure CAPTURES the value and outlives the loop; calling it after the
# break must still reach the captured region.
(defn w-capture (i)
  (let [f (forever
            (let [msg {:k (string "f" i) :n i}]
              (cond
                (= msg:n -1) (break nil)
                (= msg:k :never) (break nil)
                true (break (fn () msg:k)))))]
    (length (f))))

# (e) the loop runs several iterations before breaking. Each earlier iteration
# releases at its own point and nil-stamps the slot; the replica must free the
# last one and no-op against every earlier stamp rather than double-releasing a
# recycled region.
(defn w-many (i)
  (var n 0)
  (var last nil)
  (forever
    (let [msg {:k (string "m" i "-" n) :n n}]
      (assign n (%add n 1))
      (cond
        (= msg:k :never) (break nil)
        (%lt n 6) (assign last msg:k)
        true (break nil))))
  (length last))

# (f) the value is RETURNED through the block to a caller that reads it. The
# return mint is what the replica must leave standing.
(defn w-return-inner (i)
  (forever
    (let [msg {:k (string "r" i) :n i}]
      (cond
        (= msg:n -1) (break nil)
        (= msg:k :never) (break nil)
        true (break msg)))))
(defn w-return (i)
  (let [r (w-return-inner i)]
    (length r:k)))

# (g) a value bound OUTSIDE the loop, read inside it, with the break in a clause
# body. It is one region per activation, not per iteration, so the anchor covers
# it — and the replica must not free it a second time.
(defn w-outer (i)
  (let [outer {:k (string "o" i "-long")}]
    (forever
      (let [msg {:k (string "i" i) :n i}]
        (cond
          (= msg:k :never) (break nil)
          (= outer:k :never) (break nil)
          true (break nil))))
    (length outer:k)))

# ── control: the same read with no break — correct now (harness sanity) ───────
(defn c-plain (i)
  (let [x {:k (string "p" i)}]
    (length x:k)))

# ── drive: fresh subject each iteration; an over-early free faults on the read ─
(var i 0)
(var a 0)
(var b 0)
(var c 0)
(var d 0)
(var e 0)
(var f 0)
(var g 0)
(var k 0)
(while (%lt i 3000)
  (assign a (w-carried i))
  (assign b (w-carried-borrow i))
  (assign c (w-store i))
  (assign d (w-capture i))
  (assign e (w-many i))
  (assign f (w-return i))
  (assign g (w-outer i))
  (assign k (c-plain i))
  # The sink is a module-level container by design (witness c stores into it);
  # drain it so the driver's own retention stays flat.
  (assign sink @[])
  (assign i (%add i 1)))

(assert (%gt k 0) "control: plain struct read mis-read (harness broken)")

(assert (%gt a 0) "the value the break carried was freed under its consumer")
(assert (%gt b 0) "a borrow the break carried out was freed with its container")
(assert (%gt c 0) "a value stored into a longer-lived container was freed")
(assert (%gt d 0) "a value the escaping closure captured was freed")
(assert (%gt e 0) "an earlier iteration's release and the replica double-freed")
(assert (%gt f 0) "the returned value was freed under the caller's read")
(assert (%gt g 0) "a value bound outside the loop was freed by the replica")

(println "region-break-loop-replica-uaf: ok")
