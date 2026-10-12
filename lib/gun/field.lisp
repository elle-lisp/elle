(elle/epoch 14)
# audited: 2026-10-12
## A field of a gun node: the state of its last write and its value, built and read only here.
## lib/gun.md
## lib/gun/overview.md
##
## Loaded via:
##   (def field ((import "std/gun/field")))
##
## Exports: {:make :state :value}
##
## A field is the immutable array [state value]. Code outside this file
## builds one through make and reads it through state and value, so the
## representation can change here alone.

(fn []

  (defn make [st v]
    "The field holding value v, written at state st."
    [st v])

  (defn field-state [f]
    "The state of field f."
    (get f 0))

  (defn field-value [f]
    "The value of field f."
    (get f 1))

  {:make make :state field-state :value field-value})
