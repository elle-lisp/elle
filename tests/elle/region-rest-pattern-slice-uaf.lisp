(elle/epoch 12)
# audited: 2026-09-16
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
# THE TRAP rows 13 and 14 guard. A collection a further pattern matched is held
# by the lowerer's own slot and by no name, so its release is carried back ahead
# of a frame-replacing tail call like the scrutinee's. The names that pattern
# bound are elements of it, and the call is about to read one.
#
# THE TRAP row 15 guards. A collection the program names NOWHERE — the outer one
# of `[h & [& q]]`, and the one a bare `[& _]` builds for its type check — takes
# its release route by position alone. Both releases run with nothing keyed on a
# name to hold them back, so each must drop its own reference and no other's.
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

# ── 13. the names a NESTED rest sub-pattern bound ─────────────────────────────
# `& [p q]` builds a collection no single name holds. `p` and `q` are uncounted
# reads of THAT collection, so it must outlive both, and the strings it copied
# out of the scrutinee must survive its cascade.

(defn nested-read []
  (let [[h & [p q]] strs]
    (+ (length p) (length q))))

(var cc 0)
(while (< cc 40)
  (assert (= (nested-read) 4)
          "a name a nested rest bound must survive the collection's release")
  (assign cc (+ cc 1)))

(defn nested-return []
  (let [[h & [p q]] strs]
    p))

(var dd 0)
(while (< dd 40)
  (assert (= (nested-return) "bb")
          "a returned nested rest name must survive its collection")
  (assign dd (+ dd 1)))

(def nested-thunks @[])
(var ee 0)
(while (< ee 40)
  (let [[h & [p q]] strs]
    (push nested-thunks (fn [] (length p))))
  (assign ee (+ ee 1)))
(var ff 0)
(while (< ff 40)
  (assert (= ((get nested-thunks ff)) 2)
          "a captured nested rest name must survive the iteration that bound it")
  (assign ff (+ ff 1)))

# A rest nested inside a rest builds TWO collections. The holder set stops at
# the inner build, so the two releases are separate and neither may reach the
# other's pages.

(defn nested-in-nested []
  (let [[h & [p & q]] strs]
    (+ (length p) (length q))))

(var gg 0)
(while (< gg 40)
  (assert (= (nested-in-nested) 4)
          "two nested collections must each survive their own release")
  (assign gg (+ gg 1)))

# ── 14. a name the inner pattern bound, handed to a TAIL call ─────────────────
# THE TRAP. The call receives an ELEMENT of the collection, so the callee's
# owned-param release names the element's region and never the collection's.
# Nothing takes the collection's release over, so it is carried back ahead of
# the call — and it must not take the element the callee is about to read with
# it. Three callees, because what follows the call differs: a native returns
# into the block the release sits in, a closure replaces the frame, and a
# struct rest reaches the same relocation through `StructRest`.

(defn len-of [v]
  (length v))

(defn nested-tail-closure []
  (let [[h & [p q]] strs]
    (len-of p)))

(defn nested-tail-native []
  (let [[h & [p q]] strs]
    (length p)))

(defn nested-tail-struct []
  (let [{:a one & {:b bb}} rec]
    (+ one bb)))

(var hh 0)
(while (< hh 40)
  (assert (= (nested-tail-closure) 2)
          "a nested rest name handed to a closure tail call must survive it")
  (assert (= (nested-tail-native) 2)
          "a nested rest name handed to a native tail call must survive it")
  (assert (= (nested-tail-struct) 3)
          "a nested struct rest name handed to a tail call must survive it")
  (assign hh (+ hh 1)))

# ── 15. the collection a NAMELESS rest sub-pattern built ──────────────────────
# THE TRAP. `[h & [& q]]` builds two collections, and `q` holds the inner one.
# The outer is reached by no name of the program, so its release is pinned at
# the destructure node and nothing carries it later. `q`'s array copied the
# outer's element values, counting a reference of its own on each, so the
# outer's cascade must leave every one of them standing — and `q` outlives the
# destructure on each of the four routes below.

(defn inner-only-read []
  (let [[h & [& q]] strs]
    (+ (length q) (length (get q 0)))))

(var ii 0)
(while (< ii 40)
  (assert (= (inner-only-read) 5)
          "the inner collection must survive the outer one's release")
  (assign ii (+ ii 1)))

(defn inner-only-return []
  (let [[h & [& q]] strs]
    q))

(var jj 0)
(while (< jj 40)
  (let [got (inner-only-return)]
    (assert (= (length got) 3) "a returned inner collection must survive")
    (assert (= (get got 2) "dd") "and so must its elements"))
  (assign jj (+ jj 1)))

(def inner-thunks @[])
(var kk 0)
(while (< kk 40)
  (let [[h & [& q]] strs]
    (push inner-thunks (fn [] (length q))))
  (assign kk (+ kk 1)))
(var ll 0)
(while (< ll 40)
  (assert (= ((get inner-thunks ll)) 3)
          "a captured inner collection must survive its iteration")
  (assign ll (+ ll 1)))

(defn inner-only-tail []
  (let [[h & [& q]] strs]
    (len-of q)))

(var mm 0)
(while (< mm 40)
  (assert (= (inner-only-tail) 3)
          "an inner collection handed to a tail call must survive it")
  (assign mm (+ mm 1)))

# A bare wildcard rest keeps its build, because `ArrayMutSliceFrom` is the only
# check this pattern makes. Nothing reads what it built, so the one thing the
# release can reach wrongly is the SCRUTINEE it copied out of.

(defn bare-wildcard []
  (let [[& _] strs]
    (length strs)))

(var nn 0)
(while (< nn 40)
  (assert (= (bare-wildcard) 4) "the scrutinee must survive a discarded build")
  (assert (= (get strs 0) "aa") "and keep its elements")
  (assign nn (+ nn 1)))

(println "region-rest-pattern-slice-uaf: ok")
