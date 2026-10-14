(elle/epoch 14)
# audited: 2026-10-11
## The HAM verdict on one incoming field against the field it would replace.
## lib/gun.md
## lib/gun/overview.md
##
## Loaded via:
##   (def ham ((import "std/gun/ham") :state state :value value))
##
## Exports: {:verdict}

(fn [&named state value]

  (defn verdict [now in-state in-value cur-state cur-value]
    "The HAM verdict on a write of in-value at in-state over cur-value at
     cur-state, when the latest state a write may carry is now. A field that
     does not exist has a nil cur-state.

     :future   in-state is after now
     :stale    in-state is before cur-state
     :accept   in-state is after cur-state, or the states are equal and the
               encoding of in-value sorts after that of cur-value
     :same     otherwise"
    (cond
      (state:before? now in-state) :future
      (state:before? in-state cur-state) :stale
      (state:before? cur-state in-state) :accept
      (value:before? cur-value in-value) :accept
      :same))

  {:verdict verdict})
