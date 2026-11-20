(elle/epoch 18)
# audited: 2026-11-20
## The errors std/check raises, each built by one constructor, and the test of the error that skips a case.
## docs/stdlib/check.md
## docs/stdlib/check/overview.md

(fn []
  (defn argument-error [message]
    "Raise :argument-error with MESSAGE."
    (error {:error :argument-error :message message}))

  (defn skip []
    "Raise :check-skip, which a case reads as one that neither passes nor
     fails."
    (error {:error :check-skip :message "check: the case skips"}))

  (defn skip? [err]
    "Whether ERR is the :check-skip error `skip` raises."
    (and (struct? err) (= :check-skip (get err :error))))

  (defn gave-up [what n]
    "The :check-gave-up error of WHAT, which gave up when N cases skipped."
    {:error :check-gave-up
     :message (string "check: " n " cases skipped, so " what " gave up")})

  (defn failed-assertion [message r seed]
    "The :failed-assertion error with MESSAGE, for the result R of a run from
     SEED."
    {:error :failed-assertion
     :message message
     :value (r :value)
     :seed seed
     :bits (r :bits)})

  {:argument-error argument-error
   :skip skip
   :skip? skip?
   :gave-up gave-up
   :failed-assertion failed-assertion})
