(elle/epoch 13)
# audited: 2026-09-29
# A park a host refuses owes what a boundary's park owes.
# docs/impl/region/park.md
#
# `eval` cannot hold a suspension of the code it runs, so it refuses one with
# an error at its own call. The refused park has no reader and no install, and
# its frames never run again, so the host ends it as a squelch boundary does:
# the frames' owed releases run, and the park's delivery retain and a
# runtime-built payload are released.
#
# This file is the LEAK gauge: `arena/count` and `arena/region-count` deltas
# over a fixed window. Each `eval` compiles its form, so the gauge compares a
# refusal against a plain error raised by the same host. A refusal must cost
# no more than that.
#
# The counter-factual only clears the ledger, and the refused frames and the
# park's references go with the fiber that is dropped. Every subject then
# costs regions and objects beyond the control.

(def window 300)

(defn measure [thunk warm window]
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def objects (arena/count))
  (def regions (arena/region-count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  [(%sub (arena/count) objects) (%sub (arena/region-count) regions)])

# subjects ─────────────────────────────────────────────────────────────────────

# (a) an Emit park with a HEAP payload the evaluated code allocated.
(defn heap-yield []
  (protect (eval '(yield (string "y" 1)))))

# (b) an io park in call position: a frame is parked, and the request is the
# runtime's value.
(defn io-park []
  (protect (eval '(begin
                    (ev/sleep 0)
                    1))))

# (c) a park inside a call chain whose frames hold allocations of their own.
(defn nested-park []
  (protect (eval '((fn []
                     (let [s (string "a" 1)]
                       (+ (length s) (yield s))))))))

# (d) a BORROWED payload: the evaluated code yields a value it did not
# allocate. Nothing the refused frames owe names it, so a release beyond the
# park's delivery retain frees it under `shared`.
(def shared (string "shared-" 1))
(defn borrowed-yield []
  (protect (eval '(yield s) {'s shared})))

# control ──────────────────────────────────────────────────────────────────────

# A plain error raised by the same host: the compile and the error cost the
# same, and no park is refused.
(defn plain-error []
  (protect (eval '(error (string "y" 1)))))

# measurement ──────────────────────────────────────────────────────────────────

(def d-control (measure plain-error 20 window))
(def d-heap (measure heap-yield 20 window))
(def d-io (measure io-park 20 window))
(def d-nested (measure nested-park 20 window))
(def d-borrowed (measure borrowed-yield 20 window))

(println "region-host-refusal over " window " iters [objects regions]:")
(println "  plain error      " d-control " (control)")
(println "  heap yield       " d-heap)
(println "  io park          " d-io)
(println "  nested park      " d-nested)
(println "  borrowed yield   " d-borrowed)

(def slack 50)

(defn bounded [label delta]
  (let [objects (- (get delta 0) (get d-control 0))
        regions (- (get delta 1) (get d-control 1))]
    (begin
      (assert (< objects slack)
              (concat label ": objects beyond the control, delta="
                      (number->string objects)))
      (assert (< regions slack)
              (concat label ": regions beyond the control, delta="
                      (number->string regions))))))

(bounded "a refused heap yield releases its delivery retain" d-heap)
(bounded "a refused io park releases its request" d-io)
(bounded "the refused frames run what they owed" d-nested)
(bounded "a refused borrowed yield costs nothing" d-borrowed)

(assert (= shared "shared-1")
        "a borrowed payload survives every refusal of its park")

(println "region-host-refusal: ok")
