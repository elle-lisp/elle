(elle/epoch 18)
# audited: 2026-11-20
## The errors std/check raises, each built by one constructor.
## docs/stdlib/check.md
## docs/stdlib/check/overview.md

(fn []
  (defn argument-error [message]
    "Raise :argument-error with MESSAGE."
    (error {:error :argument-error :message message}))

  (defn skip []
    "Raise :check/skip, the error that tears down the code of a case after
     it signalled :check/skip."
    (error {:error :check/skip :message "check: the case skips"}))

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
   :gave-up gave-up
   :failed-assertion failed-assertion})
