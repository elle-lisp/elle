(elle/epoch 12)
# audited: 2026-09-16
# A rest pattern's collection is built, not read out
# (docs/impl/region/anchors.md § "A rest pattern's collection is built, not
# read out").
#
# Every other name a pattern binds is a projection of the scrutinee — an
# uncounted read that owes no release. A rest name is the one that is not:
# `[a b & r]` and `{:k v & r}` BUILD a fresh collection in a region the opcode
# mints, so the borrowing reading leaves that region with nobody to release it.
#
# THE TRAP the (a)/(c)/(e) rows guard. The rate does not depend on the rest
# name being READ. An unread name has no uses for the binding chain to extend a
# release over, so the placeholder needs a base pin of its own — the destructure
# node — or the shape that provokes the defect most often is the one the fix
# misses.
#
# THE COUNTER-FACTUAL the controls catch. The rate is flat in the scrutinee's
# length and the same for a pattern that binds the same names flatly, so an
# object-count match with the copy-scratch family proves nothing. (k) binds all
# five elements with no rest and (l) takes a LIST rest, whose cons tail is a
# borrow of the scrutinee and allocates nothing: both already read zero, so a
# subject's rate is the built collection and nothing else.
#
# THE BOUNDARY the (u) and (w) rows hold. A holder that is an operand of the
# body's frame-replacing tail call keeps its collection's exemption, and a
# `match` builds one collection per access path rather than one per
# sub-pattern. Both are stated as rows reading the FULL rate, so a change that
# reaches one of them fails here.
#
# THE COUNTER-FACTUAL the (v), (x) and (y) rows catch. A build reaches its
# placeholder by POSITION, so a collection no name of the program holds still
# takes a release route. (x) binds its one name to the INNER collection and (y)
# binds none at all; keyed on a name, each would find none and strand. (v) is
# the wildcard rest, where the cheaper answer applies and no build is emitted —
# a row that reads the full rate there says the build came back.
#
# This file is the LEAK gauge — an `arena/region-count` delta over a fixed
# window, BOUNDED for every subject. The soundness complement is
# region-rest-pattern-slice-uaf.lisp.

(def window 400)

(def arr [1 2 3 4 5])
(def marr @[1 2 3 4 5])
(def rec {:a 1 :b 2 :c 3 :d 4})
(def lst '(1 2 3 4 5))

(defn measure [thunk warm window]
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/region-count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/region-count) before))

# subjects ─────────────────────────────────────────────────────────────────────

# (a) the issue's shape: an array rest whose name the body never reads. The
# collection is built regardless, so nothing about the body can be what
# releases it.
(defn a-array-rest-unread []
  (let [[x y & r] arr]
    x))

# (b) the same pattern with the name READ. The binding chain carries the
# release over the read; the collection must still go.
(defn b-array-rest-read []
  (let [[x y & r] arr]
    (length r)))

# (c) `match` reads the pattern the same way `let` does — the decision tree
# loads the rest name by an access path ending in the slice.
(defn c-match-rest-unread []
  (match arr
    [x y & r] x))

(defn d-match-rest-read []
  (match arr
    [x y & r] (length r)))

# (e) a STRUCT rest builds a new struct of the keys the pattern did not name —
# the same defect through `StructRest` rather than `ArrayMutSliceFrom`.
(defn e-struct-rest []
  (let [{:a x & r} rec]
    x))

(defn f-match-struct-rest []
  (match rec
    {:a x & r} (length r)))

# (g) a MUTABLE `@array` rest. The built collection is a container the free
# cascade walks rather than an immutable one it scanned at allocation.
(defn g-array-mut-rest []
  (let [@[x & r] marr]
    x))

# (h) a PARAMETER destructure — the prologue's own pattern, whose scrutinee is
# an argument rather than a named binding.
(defn take-rest-pattern [[x y & r]]
  (+ x (length r)))
(defn h-param-rest []
  (take-rest-pattern arr))

# (i) the rest name handed OUT. The collection leaves on the return, so its
# release is the caller's — the row that says the fix releases an owned value
# rather than an unconditional one.
(defn hand-back []
  (let [[x & r] arr]
    r))
(defn i-rest-returned []
  (length (hand-back)))

# (j) the rest name stored into a container that is emptied each call. The
# escape is counted, so the release must drop the destructure's reference and
# leave the container's standing.
(def sink @[])
(defn j-rest-stored []
  (let [[x & r] arr]
    (push sink r)
    (pop sink)
    x))

# (n) the rest name handed to a NATIVE tail call. The release sits in the block
# the native falls through to, so it runs — if the relocation left it there.
(defn n-tail-native []
  (let [[x y & r] arr]
    (length r)))

# (o) the same through a CLOSURE tail call, which REPLACES the frame. The
# release is dead code on that path and the callee's owned parameter is what
# frees the collection, so this row reads the transfer rather than the release.
(defn count-it [v]
  (length v))
(defn o-tail-closure []
  (let [[x y & r] arr]
    (count-it r)))

# (p) the rest name and its SCRUTINEE in one argument list. The collection is a
# region the name holds and the scrutinee one it only names, so the two take
# opposite answers out of the same call.
(defn both [a b]
  (+ (length a) (length b)))
(defn take-both [t]
  (let [[x y & r] t]
    (both r t)))
(defn p-rest-and-scrutinee []
  (take-both arr))

# (q) a NESTED rest sub-pattern. `& [p q]` builds a collection no single name
# holds, and the names it binds project that collection rather than the
# scrutinee, so the placeholder is keyed on the sub-pattern and unioned into
# both of them (elle-lisp/elle#1127).
(defn q-nested-rest-unread []
  (let [[x & [p q]] arr]
    x))

(defn r-nested-rest-read []
  (let [[x & [p q]] arr]
    (let [s (+ p q)]
      s)))

# (s) the same through `StructRest`, whose inner pattern reads a key out of the
# struct the outer rest built.
(defn s-nested-struct-rest []
  (let [{:a x & {:b bb}} rec]
    (let [s (+ x bb)]
      s)))

# (t) a nested rest INSIDE a nested rest: two collections, two regions. The
# holder set stops at the inner build, so neither release covers the other and
# both must still fire.
(defn t-nested-in-nested []
  (let [[x & [p & q]] arr]
    (let [s (+ p (length q))]
      s)))

# (v) a WILDCARD rest. Nothing can read the collection this would build, so the
# lowerer emits no build at all and the rate is zero rather than one built and
# freed.
(defn v-wildcard-rest []
  (let [[x & _] arr]
    x))

# (x) a rest sub-pattern whose only name holds the INNER collection. The outer
# one is a live intermediate — the inner pattern slices it — and no name of the
# program reaches it, so only the positional key gives it a route.
(defn x-inner-only-name []
  (let [[x & [& q]] arr]
    x))

(defn x2-inner-only-name-read []
  (let [[x & [& q]] arr]
    (length q)))

# (y) a bare wildcard rest, whose pattern has no fixed element. The build stays:
# `ArrayMutSliceFrom` checks the scrutinee is an array, and this pattern makes
# that check nowhere else. So the collection is built AND released.
(defn y-bare-wildcard-rest []
  (let [[& _] arr]
    0))

# baselines ────────────────────────────────────────────────────────────────────
#
# The two shapes the release does not reach, stated as rows so a change that
# reaches one of them fails here and sends the author to anchors.md.

# (u) a name the inner pattern bound, as an operand of the body's
# frame-replacing tail call. The call receives an element, and the caller holds
# no counted reference on an element to move, so the collection's release keeps
# its exemption and strands on the closure path. Carried ahead of the call it
# would free the element the callee is about to read.
(defn take-one [v]
  (+ v 1))
(defn u-nested-tail []
  (let [[x & [p q]] arr]
    (take-one p)))

# (w) a nested rest in a `match`. The decision tree loads each name by walking
# its own access path and re-runs the `Slice` step on every path through the
# rest, so the count is a fact about the tree rather than about the pattern.
(defn w-match-nested-rest []
  (match arr
    [x y z & [p q]] x))

# controls ─────────────────────────────────────────────────────────────────────

# (k) the flat pattern: the same five elements out of the same array, bound by
# name, with no rest. The discriminator for "this is the built collection".
(defn k-flat-pattern []
  (let [[x y z w v] arr]
    x))

# (l) a LIST rest. The remaining cons tail is a pointer into the scrutinee, so
# this pattern allocates nothing and must stay at zero either way.
(defn l-list-rest []
  (let [(x y & r) lst]
    (length r)))

# measurement ──────────────────────────────────────────────────────────────────

(def d-a (measure a-array-rest-unread 20 window))
(def d-b (measure b-array-rest-read 20 window))
(def d-c (measure c-match-rest-unread 20 window))
(def d-d (measure d-match-rest-read 20 window))
(def d-e (measure e-struct-rest 20 window))
(def d-f (measure f-match-struct-rest 20 window))
(def d-g (measure g-array-mut-rest 20 window))
(def d-h (measure h-param-rest 20 window))
(def d-i (measure i-rest-returned 20 window))
(def d-j (measure j-rest-stored 20 window))
(def d-n (measure n-tail-native 20 window))
(def d-o (measure o-tail-closure 20 window))
(def d-p (measure p-rest-and-scrutinee 20 window))
(def d-q (measure q-nested-rest-unread 20 window))
(def d-r (measure r-nested-rest-read 20 window))
(def d-s (measure s-nested-struct-rest 20 window))
(def d-t (measure t-nested-in-nested 20 window))
(def d-u (measure u-nested-tail 20 window))
(def d-v (measure v-wildcard-rest 20 window))
(def d-w (measure w-match-nested-rest 20 window))
(def d-x (measure x-inner-only-name 20 window))
(def d-x2 (measure x2-inner-only-name-read 20 window))
(def d-y (measure y-bare-wildcard-rest 20 window))
(def d-k (measure k-flat-pattern 20 window))
(def d-l (measure l-list-rest 20 window))

# (m) the issue's own shape, INLINE in the driving loop rather than inside a
# thunk the driver calls. A release hoisted to a loop node fires once per CALL
# in the thunk form and reads zero there while the inline form reads the full
# rate, so the subject is measured both ways.
(def m-before (arena/region-count))
(var m 0)
(while (%lt m window)
  (let [[x y & r] arr]
    x)
  (assign m (%add m 1)))
(def d-m (%sub (arena/region-count) m-before))

(println "region-rest-pattern-slice over " window " iters (region deltas):")
(println "  a array-rest-unread " d-a)
(println "  b array-rest-read   " d-b)
(println "  c match-rest-unread " d-c)
(println "  d match-rest-read   " d-d)
(println "  e struct-rest       " d-e)
(println "  f match-struct-rest " d-f)
(println "  g array-mut-rest    " d-g)
(println "  h param-rest        " d-h)
(println "  i rest-returned     " d-i)
(println "  j rest-stored       " d-j)
(println "  n tail-native       " d-n)
(println "  o tail-closure      " d-o)
(println "  p rest+scrutinee    " d-p)
(println "  q nested-unread     " d-q)
(println "  r nested-read       " d-r)
(println "  s nested-struct     " d-s)
(println "  t nested-in-nested  " d-t)
(println "  v wildcard-rest     " d-v)
(println "  x inner-only-name   " d-x)
(println "  x2 inner-only-read  " d-x2)
(println "  y bare-wildcard     " d-y)
(println "  u nested-tail       " d-u " (baseline)")
(println "  w match-nested      " d-w " (baseline)")
(println "  k flat-pattern      " d-k " (control)")
(println "  l list-rest         " d-l " (control)")
(println "  m inline-loop       " d-m)

(assert (%lt d-k 40)
        (concat "control: a flat array pattern must be bounded, delta="
                (number->string d-k)))
(assert (%lt d-l 40)
        (concat "control: a list rest borrows the cons tail and must be "
                "bounded, delta=" (number->string d-l)))

(assert (%lt d-a 40)
        (concat "an unread array rest name strands its collection, delta="
                (number->string d-a)))
(assert (%lt d-b 40)
        (concat "a read array rest name strands its collection, delta="
                (number->string d-b)))
(assert (%lt d-c 40)
        (concat "an unread `match` rest name strands its collection, delta="
                (number->string d-c)))
(assert (%lt d-d 40)
        (concat "a read `match` rest name strands its collection, delta="
                (number->string d-d)))
(assert (%lt d-e 40)
        (concat "a struct rest name strands its collection, delta="
                (number->string d-e)))
(assert (%lt d-f 40)
        (concat "a `match` struct rest name strands its collection, delta="
                (number->string d-f)))
(assert (%lt d-g 40)
        (concat "an `@array` rest name strands its collection, delta="
                (number->string d-g)))
(assert (%lt d-h 40)
        (concat "a parameter pattern's rest name strands its collection, "
                "delta=" (number->string d-h)))
(assert (%lt d-i 40)
        (concat "a returned rest collection strands at its consumer, delta="
                (number->string d-i)))
(assert (%lt d-j 40)
        (concat "a stored rest collection strands the destructure's reference, "
                "delta=" (number->string d-j)))
(assert (%lt d-m 40)
        (concat "an inline rest destructure strands its collection, delta="
                (number->string d-m)))
(assert (%lt d-n 40)
        (concat "a rest collection tail-called into a native strands, delta="
                (number->string d-n)))
(assert (%lt d-o 40)
        (concat "a rest collection tail-called into a closure strands, delta="
                (number->string d-o)))
(assert (%lt d-p 40)
        (concat "a rest collection passed beside its scrutinee strands, delta="
                (number->string d-p)))
(assert (%lt d-q 40)
        (concat "an unread nested rest sub-pattern strands its collection, "
                "delta=" (number->string d-q)))
(assert (%lt d-r 40)
        (concat "a read nested rest sub-pattern strands its collection, delta="
                (number->string d-r)))
(assert (%lt d-s 40)
        (concat "a nested struct rest strands its collection, delta="
                (number->string d-s)))
(assert (%lt d-t 40)
        (concat "a rest nested inside a rest strands one of the two "
                "collections, delta=" (number->string d-t)))
(assert (%lt d-v 40)
        (concat "a wildcard rest builds a collection nothing can read, delta="
                (number->string d-v)))
(assert (%lt d-x 40)
        (concat "a rest whose only name holds the inner collection strands the "
                "outer one, delta=" (number->string d-x)))
(assert (%lt d-x2 40)
        (concat "reading the inner name strands the outer collection, delta="
                (number->string d-x2)))
(assert (%lt d-y 40)
        (concat "a bare wildcard rest strands the collection its type check "
                "builds, delta=" (number->string d-y)))
# The baselines read the FULL rate — one region per iteration at least. A row
# that drops below it means the shape is covered now, which anchors.md says it
# is not; repair the document and move the row up to a subject.
(assert (>= d-u window)
        (concat "baseline: a nested rest name is an operand of the body's tail "
                "call, delta=" (number->string d-u)))
(assert (>= d-w window)
        (concat "baseline: a `match` nested rest re-slices per access path, "
                "delta=" (number->string d-w)))

(println "region-rest-pattern-slice: ok")
