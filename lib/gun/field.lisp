(elle/epoch 15)
# audited: 2026-10-21
## A field of a gun node: its state, its value, and its signature in a user space, built and read only here.
## lib/gun.md
## lib/gun/sea.md
## lib/gun/overview.md
##
## Loaded via:
##   (def field ((import "std/gun/field")))
##
## Exports: {:make :state :value :signature}
##
## A field is the immutable array [state value], or [state value signature]
## when it belongs to a user space. Code outside this file builds one through
## make and reads it through the accessors, so the representation can change
## here alone.

(fn []

  (defn make [st v &opt sig]
    "The field holding value v, written at state st, signed by the bytes sig
     when sig is given."
    (if (nil? sig) [st v] [st v sig]))

  (defn field-state [f]
    "The state of field f."
    (get f 0))

  (defn field-value [f]
    "The value of field f."
    (get f 1))

  (defn signature [f]
    "The signature of field f, or nil when it carries none."
    (when (> (length f) 2) (get f 2)))

  {:make make :state field-state :value field-value :signature signature})
