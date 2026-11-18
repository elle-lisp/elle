(elle/epoch 15)
# audited: 2026-11-17
# A tail argument that is a `let` over a borrowed value is minted for the callee as the value itself is.
# docs/impl/region/rules.md
#
# The sidecar arms guardfree, so a released page is unmapped and a stale read
# faults at its deref.
#
# `(sink (let [a k] a))` hands the callee `k`, and the `let` counts no read of
# it, so the frame holds no reference of its own to move. `sink` releases its
# parameter, and that release has to be the mint the call made.
#
# The counter-factual: a call that read the `let` as an owned argument moved a
# reference it never held. Each call took one count of `k` away from whatever
# held it, and the read after enough calls faults on the freed page. The hoist
# builds this shape from a literal a grafted body reads, so a closure called in
# a loop reached it (region-tail-operand-let.lisp).

(def flag (> (length (sys/argv)) 100))
(def rounds 400)

# A callee the graft leaves a call, so the tail call replaces the frame.
(defn sink [h]
  (block :keep
    (length h)))

(defn drive [thunk]
  "Call thunk rounds times, and answer its last value."
  (var @i 0)
  (var @last nil)
  (while (%lt i rounds)
    (assign last (thunk))
    (assign i (%add i 1)))
  last)

# ── 1. a top-level binding, through a let ───────────────────────────────

(def k (array 1 2))
(assert (= (drive (fn []
                    (sink (let [a k]
                            a)))) 2) "a let over a global")
(assert (= (drive (fn []
                    (sink (let [a k]
                            (if flag a a))))) 2)
        "a let over a global, through both arms")
(assert (= (drive (fn []
                    (sink (let [a k]
                            (begin
                              (length a)
                              a))))) 2) "a let over a global, through a begin")
(assert (= k [1 2]) "the global keeps its value")

# ── 2. a captured upvalue, through a let ────────────────────────────────

(defn holder [s]
  "A tick that hands s to sink through a let, and a peek at s."
  {:tick (fn []
           (sink (let [a s]
                   a)))
   :peek (fn [] (get s 0))})

(let [h (holder (array 7 8 9))]
  (assert (= (drive h:tick) 3) "a let over a capture")
  (assert (= (h:peek) 7) "the capture keeps its value"))

(println "region-tail-let-borrow-uaf: ok")
