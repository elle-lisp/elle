(elle/epoch 16)
# audited: 2026-11-13
# fiber/signaled? reads a fiber's last signal by keyword, so no program writes a signal's bit number.
# docs/signals/primitives.md
# docs/signals/protocol.md

# A yield raises :yield and nothing else. The counter-factual answered from
# the fiber's status, which reads :paused for a yield and a caught error alike.
(let [gen (fiber/new (fn []
                       (yield 1)
                       :done) |:yield|)]
  (assert (not (fiber/signaled? gen :yield)) "a new fiber has raised nothing")
  (fiber/resume gen)
  (assert (fiber/signaled? gen :yield) "a yield raises :yield")
  (assert (not (fiber/signaled? gen :error)) "and not :error")
  (fiber/resume gen)
  (assert (not (fiber/signaled? gen :yield)) "a return raises nothing"))

# A caught error leaves the fiber :paused, as a yield does; only the error
# bit tells the two apart.
(let [f (fiber/new (fn [] (error :boom)) |:error|)]
  (fiber/resume f)
  (assert (= (fiber/status f) :paused) "a caught error pauses the fiber")
  (assert (fiber/signaled? f :error) "and the fiber raised :error"))

# A cancel leaves the fiber :dead, as a return does, and holds the error bit.
(let [f (fiber/new (fn [] (yield 1)) |:yield|)]
  (fiber/resume f)
  (fiber/cancel f :gone)
  (assert (fiber/signaled? f :error) "a cancel holds the error bit"))

# A set asks for any of its keywords, as a mask catches on any shared bit.
# The counter-factual asked for every keyword, a subset test.
(let [gen (fiber/new (fn [] (yield 1)) |:yield|)]
  (fiber/resume gen)
  (assert (fiber/signaled? gen |:io :yield|) "a set matches on one shared bit")
  (assert (not (fiber/signaled? gen |:io :error|)) "and not on none")
  (assert (not (fiber/signaled? gen ||)) "an empty set matches nothing"))

# An I/O request raises :io without :yield (docs/signals/protocol.md).
(let [f (fiber/new (fn []
                     (port/write (port/stdout) "")
                     :written) |:io|)]
  (fiber/resume f)
  (assert (fiber/signaled? f :io) "an I/O request raises :io")
  (assert (not (fiber/signaled? f :yield)) "and not :yield")
  (fiber/resume f 0))

# A user signal's bit is the registry's to choose, so a program can name it
# and no other way. A compound emit keeps both halves.
(signal :fiber-signaled/probe)
(let [f (fiber/new (fn [] (emit |:yield :fiber-signaled/probe| :data))
                   |:yield :fiber-signaled/probe|)]
  (fiber/resume f)
  (assert (fiber/signaled? f :fiber-signaled/probe)
          "a user signal reads by name")
  (assert (fiber/signaled? f :yield)
          "beside the built-in bit it was raised with"))

# The answer is the registry's bit, the one fiber/bits reports.
(let [gen (fiber/new (fn [] (yield 1)) |:yield|)
      bit (bit/shift-left 1 (get (signals) :yield))]
  (fiber/resume gen)
  (assert (= (fiber/signaled? gen :yield)
             (not (= 0 (bit/and (fiber/bits gen) bit))))
          "the predicate agrees with the bit the registry names"))

# A keyword the registry does not hold is an error. The counter-factual
# answered false, so a misspelled signal read as one never raised.
(let [gen (fiber/new (fn [] (yield 1)) |:yield|)]
  (fiber/resume gen)
  (let [[ok? err] (protect (fiber/signaled? gen :yeild))]
    (assert (not ok?) "an unknown keyword raises")
    (assert (= (get err :error) :signal-error) "a signal-error")))

(let [[ok? err] (protect (fiber/signaled? 42 :yield))]
  (assert (not ok?) "a value that is not a fiber raises")
  (assert (= (get err :error) :type-error) "a type-error"))
