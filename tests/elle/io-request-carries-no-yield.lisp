(elle/epoch 12)
## signals/io-request-carries-no-yield — an I/O request raises `|:io|` alone,
## so a `|:yield|` mask does not catch it.
##
## `:yield` means one thing: the cooperative suspension `(yield v)` raises. It
## used to be OR-ed onto every scheduler-bound request as well, which made a
## mask naming it unable to say which of the two it wanted.
##
## THE TRAP: an I/O request suspends its fiber, so it looks like it ought to
## carry `:yield`. Suspension does not come from that bit — it comes from
## raising any signal at all (`signals::dispatch::is_suspending`). The bit is
## what a mask matches on, and nothing else.
##
## COUNTER-FACTUAL: `fiber/bits` is asserted rather than just the fiber's value,
## because a stray `:yield` on the request changes nothing a program can see
## until some mask names `:yield` and starts swallowing scheduler traffic. The
## generator case below is that failure made observable.

# A fiber whose body does I/O, masked to catch ONLY the request.
(let [f (fiber/new (fn []
                     (ev/sleep 0.001)
                     :done) |:io :error|)]
  (fiber/resume f)
  (assert (= (fiber/bits f) 512)
          (string "an io request raises |:io| alone — got " (fiber/bits f))))

# The generator shape the whole rule exists to protect: `port/lines`,
# `tls/lines`, and the SSE streams in lib/http.lisp all mask `|:yield|` around a
# body that does I/O. The `(yield v)` must be caught here; the I/O must not be,
# or it never reaches the scheduler and the read never completes.
(let [g (fiber/new (fn []
                     (ev/sleep 0.001)
                     (yield :from-generator)
                     :done) |:yield|)]
  (fiber/resume g)
  (assert (= (fiber/bits g) 2)
          (string "a |:yield| mask catches the yield, not the io — got "
                  (fiber/bits g)))
  (assert (= (fiber/value g) :from-generator)
          "the value the generator yielded reaches its consumer"))

(println "io-request-carries-no-yield: ok")
