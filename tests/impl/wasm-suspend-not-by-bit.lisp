(elle/epoch 12)
## wasm/suspend-not-by-bit — a suspending signal that does not carry `:yield`
## still captures a continuation, and does not pick up a transport bit.
##
## THE TRAP: emitted WASM decided "did my callee suspend?" by testing bit 1 of
## the callee's signal word (`signal & 2`, for SIG_YIELD). A suspending signal
## without that bit fell through to the error path: no spill, no `rt_yield`, no
## continuation frame. The host then OR-ed SIG_YIELD back on to mean "suspended",
## so the fiber parked — with the code after the suspend already discarded, and
## with the transport bit visible in `fiber/bits`.
##
## COUNTER-FACTUAL, and why `fiber/bits` is asserted as well as the value: on the
## VM this reports 512 and `(resumed 99)`; the WASM tier reported 514 and plain
## `99`. Asserting only the bits would miss the dropped continuation, and
## asserting only the value would miss the leaked bit. Both, or neither is caught.
##
## `emit` needs a NON-LITERAL signal argument. A literal `(emit |:io| …)` is
## recognized at compile time and lowers to an `Emit` terminator, which reports
## suspension through the `status` word and was never affected. Only a native
## call through `rt_call` reaches the bit test.

(def sig |:io|)

(defn suspend-then-continue []
  (let [r (emit sig :req)]
    (list :resumed r)))

(let [f (fiber/new suspend-then-continue |:io|)]
  (fiber/resume f)
  (assert (= (fiber/bits f) 512)
          (string "a bare |:io| suspension reports |:io|, with no transport bit "
                  "riding along — got " (fiber/bits f)))
  (assert (= (fiber/status f) :paused) "the fiber parks on the suspension")
  (let [final (fiber/resume f 99)]
    (assert (= final (list :resumed 99))
            (string "the continuation after the suspending call runs on resume "
                    "— got " final))))

(println "wasm-suspend-not-by-bit: ok")
