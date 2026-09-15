(elle/epoch 12)
# audited: 2026-09-15
# Soundness complement of region-rest-pattern-slice.lisp
# (docs/impl/region/anchors.md § "A rest pattern's collection is built, not
# read out"). Run under `--trace=guardfree` by the subprocess pin
# `region_rest_pattern_slice_uaf` in tests/integration/elle_scripts.rs.
#
# A rest name now owns the collection its pattern built and releases it at the
# name's last use. That release runs on a path that ran none before, so it owes
# what any new release owes: the reference it drops must be the destructure's
# own and nobody else's.
#
# THE TRAP behind rows 5 and 6. The built collection holds COPIES of the
# scrutinee's element values, so its allocation increfed each element's region
# and its free cascades those references away. An element read back out of the
# collection is an uncounted borrow of it, and the scrutinee is a separate
# holder the cascade must leave alone.
#
# THE COUNTER-FACTUAL row 8 catches. A `match` arm builds its rest collection
# before its guard runs, so an arm that then fails the guard leaves one behind.
# Anchoring the release inside the arm body releases it on the path that used
# it and strands it on the path that did not; anchoring it at the `match` node
# covers both — and must not reach the NEXT arm's collection, which is a
# different region with a slot of its own.
#
# Every read below happens after the release for that collection has run, so an
# over-release faults at the deref (SIGSEGV under guardfree) or trips the
# generation check.

(def src [10 20 30 40 50])
(def rec {:a 1 :b 2 :c 3 :d 4})

# ── 1. read after the destructure, in the same scope ──────────────────────────

(defn read-after [tag]
  (let [[x y & r] src]
    (def n (length r))
    (assert (= n 3) "the rest collection must survive its own scope")
    (+ n tag)))

(var i 0)
(while (< i 40)
  (read-after i)
  (assign i (+ i 1)))

# ── 2. returned out of the function ───────────────────────────────────────────
# The collection leaves on the return mint, so the callee's release must drop
# the frame's reference and not the caller's.

(defn hand-back [xs]
  (let [[x & r] xs]
    r))

(var j 0)
(while (< j 40)
  (let [got (hand-back src)]
    (assert (= (length got) 4) "a returned rest collection must survive")
    (assert (= (get got 0) 20) "its elements must survive with it"))
  (assign j (+ j 1)))

# ── 3. stored into a container that outlives the loop ─────────────────────────
# The store counts a reference of its own; the destructure's release must leave
# it standing.

(def kept @[])
(var k 0)
(while (< k 40)
  (let [[x & r] src]
    (push kept r))
  (assign k (+ k 1)))
(assert (= (length kept) 40) "every stored rest collection must still be there")
(assert (= (get (get kept 0) 0) 20)
        "a stored rest collection's elements must survive the loop")
(assert (= (length (get kept 39)) 4)
        "the last stored rest collection must survive")

# ── 4. captured by a closure called after the loop ────────────────────────────

(def thunks @[])
(var m 0)
(while (< m 40)
  (let [[x & r] src]
    (push thunks (fn [] (length r))))
  (assign m (+ m 1)))
(var n 0)
(while (< n 40)
  (assert (= ((get thunks n)) 4)
          "a captured rest collection must survive the iteration that built it")
  (assign n (+ n 1)))

# ── 5. an element borrowed out of the collection ──────────────────────────────
# `(get r 0)` is an uncounted read of the built collection, so the collection is
# used for as long as that element is.

(def strs ["aa" "bb" "cc" "dd"])
(defn borrow-element []
  (let [[h & r] strs]
    (let [e (get r 0)]
      (length e))))

(var p 0)
(while (< p 40)
  (assert (= (borrow-element) 2)
          "an element borrowed out of a rest collection must survive")
  (assign p (+ p 1)))

# ── 6. the scrutinee, which the collection only copied out of ─────────────────
# Freeing the collection cascades one reference off each element's region. The
# scrutinee holds its own, so it must read the same afterwards.

(var q 0)
(while (< q 40)
  (let [[a & r] strs]
    (assert (= (length r) 3) "the collection"))
  (assert (= (get strs 1) "bb") "the scrutinee must survive the collection")
  (assert (= (length strs) 4) "and keep its length")
  (assign q (+ q 1)))

# ── 7. carried across a fiber yield ───────────────────────────────────────────
# The park counts the payload's region, so the body's own release must not take
# the resumer's reference with it.

(def gen
  (fiber/new (fn []
               (var t 0)
               (while (< t 20)
                 (let [[a & r] src]
                   (emit :yield r))
                 (assign t (+ t 1)))
               :done) |:yield|))

(var u 0)
(while (< u 20)
  (let [got (fiber/resume gen nil)]
    (assert (= (length got) 4) "a yielded rest collection must survive the park")
    (assert (= (get got 3) 50) "and so must its elements"))
  (assign u (+ u 1)))

# ── 8. a `match` arm that builds a collection and then fails its guard ────────
# The first arm's collection is built before the guard runs. The second arm's
# is a different region with a slot of its own, so releasing the first must not
# reach it.

(defn pick [xs]
  (match xs
    [a & r] when
    (> a 100) (length r)
    [a b & r] (+ 1000 (length r))
    _ 0))

(var v 0)
(while (< v 40)
  (assert (= (pick src) 1003)
          "the arm after a failed guard must read its own rest collection")
  (assign v (+ v 1)))

# ── 9. a struct rest, read after the destructure ──────────────────────────────

(defn struct-rest-read []
  (let [{:a one & r} rec]
    (+ one (length r))))

(var w 0)
(while (< w 40)
  (assert (= (struct-rest-read) 4) "a struct rest collection must survive")
  (assign w (+ w 1)))

# ── 10. broken out of a loop ──────────────────────────────────────────────────
# The break hands the collection to the block, so the release is anchored where
# the block's value is consumed rather than inside the body.

(defn break-out []
  (block :found
    (var z 0)
    (while (< z 8)
      (let [[a & r] src]
        (when (= z 3) (break :found r)))
      (assign z (+ z 1)))
    nil))

(var y 0)
(while (< y 40)
  (let [got (break-out)]
    (assert (= (length got) 4)
            "a rest collection broken out of a loop must survive"))
  (assign y (+ y 1)))

# ── 11. handed to a TAIL call ─────────────────────────────────────────────────
# THE TRAP. A release is carried back ahead of a frame-replacing tail call
# unless the call itself names the region, and for a name a pattern bound that
# question is answered by comparing the region's release route against the
# slots the call passes. The collection's route is the slot the lowerer parked
# it in; the call passes the binding's own slot. Read off those two slots alone
# the rest name looks like a leaf the call does not move, and the release runs
# before the callee ever reads the collection.
#
# Three callees, because what follows the call differs. A native tail call
# returns into the block the release sits in, a closure one replaces the frame,
# and a struct rest reaches the same relocation through `StructRest`.

(defn tail-native []
  (let [[x y & r] src]
    (length r)))

(defn count-it [v]
  (length v))

(defn tail-closure []
  (let [[x y & r] src]
    (count-it r)))

(defn tail-struct []
  (let [{:a one & r} rec]
    (length r)))

(var aa 0)
(while (< aa 40)
  (assert (= (tail-native) 3)
          "a rest collection handed to a native tail call must survive it")
  (assert (= (tail-closure) 3)
          "a rest collection handed to a closure tail call must survive it")
  (assert (= (tail-struct) 3)
          "a struct rest collection handed to a tail call must survive it")
  (assign aa (+ aa 1)))

# ── 12. the rest name and its scrutinee in one argument list ──────────────────
# THE TRAP. The exemption is reconsidered per ARGUMENT, but the slot test it
# makes reads the whole operand list. The scrutinee's region survives the rest
# name's reconsideration only because the scrutinee is itself passed here; asked
# of one argument in isolation, it is freed before the callee reads it.

(defn sum-both [a b]
  (+ (length a) (length b)))

(defn rest-and-scrutinee [t]
  (let [[x y & r] t]
    (sum-both r t)))

(var bb 0)
(while (< bb 40)
  (assert (= (rest-and-scrutinee src) 8)
          "a rest collection and its scrutinee must both survive one call")
  (assign bb (+ bb 1)))

(println "region-rest-pattern-slice-uaf: ok")
