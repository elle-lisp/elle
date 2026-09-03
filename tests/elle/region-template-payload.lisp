(elle/epoch 12)
## region/template-payload — a code object's payload outlives the frame that
## built the closure.
##
## A closure template is now three things (docs/impl/region/template.md): a
## compile-time blueprint, a `CodePayload` of region pages holding the bytecode,
## constants, docstring and source-location table, and a per-creation header
## naming that payload. The payload is shared, so it lives in a region of the
## heap's own — NOT in the region the header was born in.
##
## The trap: the header carries the payload as a `RegionSlice`, a bare
## `(ptr, len)` pair. Nothing in the header's own bytes says another region owns
## the pages behind it, so the payload backing must be recorded as a counted
## cross-region reference at the header's allocation. Counter-factual (RED
## without that edge): the returned closure's frame is freed at its
## `decref_point`, the payload region's count never rose to cover the escaped
## header, and calling the closure reads freed pages — bytecode, docstring, and
## location table all torn.
##
## Every probe below returns a closure from a call that has ended, then reads
## something that lives only in the payload.

# ── the bytecode is payload: calling the escaped closure must still run ──
(defn make-adder (n)
  (fn [x] (+ x n)))

(let [add5 (make-adder 5)]
  (assert (= (add5 3) 8)
          "an escaped closure reads its bytecode from the payload"))

# Many closures from ONE blueprint, all outliving their frames. They share one
# payload, so every header must hold its own reference to it: releasing the
# first frame's region must not take the payload the others still read.
(let [adders (map make-adder [1 2 3 4 5 6 7 8])]
  (assert (= (map (fn [f] (f 10)) adders) [11 12 13 14 15 16 17 18])
          "each header from one blueprint holds its own payload reference"))

# ── the docstring is payload ──
(defn documented (x)
  "the docstring rides the payload, not the header"
  x)

(assert (= (doc documented) "the docstring rides the payload, not the header")
        "a docstring read after definition comes out of the payload")

# ── the source-location table is payload ──
# An error raised inside an escaped closure formats its location from the
# payload's location table. Reading a freed table gives no location or a torn
# one; asserting only that the error arrives keeps this independent of the
# message format, which this test does not own.
(defn make-failer ()
  (fn [] (error :boom)))

(let [f (make-failer)]
  (assert (= (try
               (f)
               (catch e :caught)) :caught)
          "an escaped closure's error path reads its location table"))

# ── the capture-locals mask is payload, and it is not a u64 ──
# A body with more than 64 locals, one of which is captured by a nested closure,
# forces a mask wider than one word. An uncaptured local at any index must get a
# bare-NIL env slot, so a truncated mask shows up as a wrong answer here.
(defn wide-locals ()
  (let [a0 0
        a1 1
        a2 2
        a3 3
        a4 4
        a5 5
        a6 6
        a7 7
        a8 8
        a9 9
        b0 10
        b1 11
        b2 12
        b3 13
        b4 14
        b5 15
        b6 16
        b7 17
        b8 18
        b9 19
        c0 20
        c1 21
        c2 22
        c3 23
        c4 24
        c5 25
        c6 26
        c7 27
        c8 28
        c9 29
        d0 30
        d1 31
        d2 32
        d3 33
        d4 34
        d5 35
        d6 36
        d7 37
        d8 38
        d9 39
        e0 40
        e1 41
        e2 42
        e3 43
        e4 44
        e5 45
        e6 46
        e7 47
        e8 48
        e9 49
        f0 50
        f1 51
        f2 52
        f3 53
        f4 54
        f5 55
        f6 56
        f7 57
        f8 58
        f9 59
        g0 60
        g1 61
        g2 62
        g3 63
        g4 64
        g5 65
        g6 66
        g7 67
        g8 68
        g9 69
        captured 70]
    (fn [] (+ captured g9))))

(assert (= ((wide-locals)) 139)
        "a capture past slot 64 survives the payload's multi-word mask")

# ── the &named key set is payload ──
# `&named` compiles to a strict-struct collector whose declared key set the call
# validates against. That key set is variable-length, so it lives in the payload.
(defn named-args [&named alpha]
  alpha)

(assert (= (named-args :alpha 3) 3)
        "a declared &named key is accepted from the payload's key set")

# ── the payload cache is bounded ──
# Every `eval` compiles a fresh blueprint tree, and each blueprint's payload is
# materialized into a payload region the heap owns. Those regions are released
# when the last blueprint packed into one dies, so a program that evals in a
# loop must not accumulate them.
#
# Counter-factual (RED without the cache's sweep): the payload regions are held
# to teardown and the live region count climbs one per few evals — a REPL
# session would grow without bound.
(defn eval-churn (n)
  (var i 0)
  (while (< i n)
    (eval '(fn [x] (+ x 1)))
    (assign i (+ i 1))))

(eval-churn 50)  # warm up: the first pass mints the steady-state regions
(let [before (arena/region-count)]
  (eval-churn 500)
  (assert (<= (arena/region-count) before)
          (string "500 evals must not grow the live region count (before "
                  before ", after " (arena/region-count) ")")))

(println "region-template-payload: ok")
