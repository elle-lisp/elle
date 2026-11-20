(elle/epoch 18)
# audited: 2026-11-20
## A case: the value a generator makes from one source, and whether the property passes, fails or skips on it.
## docs/stdlib/check.md
## docs/stdlib/check/overview.md
## docs/stdlib/check/functions.md
##
## A case is a struct: the `:value` the generator made, `:skipped`,
## `:failed-in`, which is `:generator`, `:property` or nil, the `:error` the
## case raised, `:bits`, every bit its draws recorded, `:choices`, the choice
## of each draw, `:record`, the source's record, and `:calls`, the log of each
## function the case made. A case whose generator raised has no value. A case
## keeps every bit of every draw: a draw reads its most significant bit first,
## so a case without its trailing zeros would rank 512 of a 10-bit draw
## simpler than 1.

(fn [&named errors gen]
  (defn finish [thunk]
    "Run THUNK in a fiber that catches :check/skip and :error, and resume it
     after each skip until it ends. [skipped? returned? v]: whether it
     signalled :check/skip, whether it returned, and what it returned or
     raised."
    (let [f (fiber/new thunk |:check/skip :error|)
          @v (fiber/resume f)
          @skipped false]
      (while (fiber/signaled? f :check/skip)
        (assign skipped true)
        (assign v (fiber/resume f)))
      [skipped (= :dead (fiber/status f)) v]))

  (defn run-case [g prop src]
    "Make a value of G from SRC and test PROP on it. A case skips when G or
     PROP signals :check/skip, whatever it does after. Else it fails in the
     generator when G raises, and in the property when PROP returns false or
     nil, or raises. :calls holds the log of each function made while G and
     PROP ran, as it stood when the case ended."
    (let [logs @[]]
      (parameterize ((gen:call-logs logs))
        (let* [[gen-skipped made? v] (finish (fn [] (g (src :draw))))
               made? (and made? (not gen-skipped))
               [prop-skipped held? result] (if made?
                                             (finish (fn [] (prop v)))
                                             [false true true])
               skipped (or gen-skipped prop-skipped)
               record ((src :record))]
          {:value (if made? v nil)
           :skipped skipped
           :failed-in (cond
                        skipped nil
                        (not made?) :generator
                        (or (not held?) (not result)) :property
                        true nil)
           :error (cond
                    skipped nil
                    (not made?) v
                    (not held?) result
                    true nil)
           :bits (record :bits)
           :choices (record :choices)
           :record record
           :calls (freeze (map freeze logs))}))))

  {:run-case run-case})
