(elle/epoch 12)
# A boundary ends a park with no reader and no install
# (docs/impl/region/owner.md § "A boundary ends a park with no reader and no
# install, so it owes both references").
#
# A `squelch`/`attune` violation is the third way a park can end. The other two
# each consume one of the park's two references: a resumer reads the payload out
# of `fiber.signal` and releases it there (the delivery reference the escape
# retain minted), and the install that replaces the payload releases what a
# RUNTIME-built payload's allocation left. A boundary is neither, so both
# references are its to release — and a body-allocated payload's own reference
# stays the abandoned frames' release tables' to run.
#
# This file is the LEAK gauge — `arena/count` and `arena/region-count` deltas
# over a fixed window, BOUNDED for every subject. The soundness complement is
# region-boundary-park-uaf.lisp.
#
# The trap: an `(emit :yield 1)` under a boundary reads bounded whatever this
# mechanism does, because an immediate payload lives in no region at all. That
# is the shape region-squelch-unwind.lisp already gauges, so this class hid
# behind a green neighbour — every subject below carries a HEAP payload.

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

(defn caught [f]
  (fn []
    (try
      (f)
      (catch e nil))))

# subjects ─────────────────────────────────────────────────────────────────────

# (a) an IO park: the request is the runtime's value, so nothing the body ever
# named holds it and the frames' release tables can name nothing either. Both
# references are the boundary's — the allocation's and the delivery retain's.
(def io-body (attune |:error| (fn [] (println ""))))

# (b) the portless io op, reached through the complementary mask. The rate is
# per park, not per port.
(def sleep-body (attune |:error| (fn [] (ev/sleep 0))))

# (c) the same io park behind a `squelch` of `:io` rather than an `attune`:
# one boundary, two spellings.
(def squelch-io-body (squelch (fn [] (println "")) :io))

# (d) an `Emit` node's park with a HEAP payload. The body allocated it, so its
# own reference is the release table's and only the delivery retain is owed.
(def emit-body (squelch (fn [] (emit :yield (string "y" 1))) :yield))

# (e) the same park reached through the emit PRIMITIVE, whose first argument the
# compiler cannot read as a keyword set. A different park site, the same debt.
(def dynamic-emit-body
  (squelch (fn [] (fiber/emit :yield (string "y" 1))) :yield))

# controls ─────────────────────────────────────────────────────────────────────

# (f) an IMMEDIATE emit payload. It lives in no region, so the boundary owes it
# nothing — and a subject built this way would read bounded before the fix.
(def immediate-body (squelch (fn [] (emit :yield 1)) :yield))

# (g) the same io call with NO boundary: the resume consumes the delivery and
# the install releases the request, so this pins that the subjects measure the
# boundary and not the io op.
(defn no-boundary []
  (println ""))

# (h) the same squelched body that never violates: the ordinary releases run.
(def clean-body (squelch (fn [] (string "y" 1)) :yield))

# (i) a RAISE at a boundary rather than a park. An error emit takes the same
# escape retain but parks nothing, and its delivery is the raise's own mint —
# recorded in the ledger, which withdraws the payload exemption so the frames'
# tables reclaim it. A regression here is the boundary claiming a raise.
(def raise-body (squelch (fn [] (error (string "y" 1))) :error))

# measurement ──────────────────────────────────────────────────────────────────

(def d-io (measure (caught io-body) 20 window))
(def d-sleep (measure (caught sleep-body) 20 window))
(def d-squelch-io (measure (caught squelch-io-body) 20 window))
(def d-emit (measure (caught emit-body) 20 window))
(def d-dynamic (measure (caught dynamic-emit-body) 20 window))
(def d-immediate (measure (caught immediate-body) 20 window))
(def d-none (measure no-boundary 20 window))
(def d-clean (measure (caught clean-body) 20 window))
(def d-raise (measure (caught raise-body) 20 window))

(println "region-boundary-park over " window " iters [objects regions]:")
(println "  io-park          " d-io)
(println "  sleep-park       " d-sleep)
(println "  squelch-io-park  " d-squelch-io)
(println "  emit-park        " d-emit)
(println "  dynamic-emit     " d-dynamic)
(println "  immediate        " d-immediate " (control)")
(println "  no-boundary      " d-none " (control)")
(println "  no-violation     " d-clean " (control)")
(println "  raise            " d-raise " (control)")

(def slack 50)

(defn bounded [label delta]
  (let [objects (get delta 0)
        regions (get delta 1)]
    (begin
      (assert (< objects slack)
              (concat label ": objects grew, delta=" (number->string objects)))
      (assert (< regions slack)
              (concat label ": regions grew, delta=" (number->string regions))))))

(bounded "control: an immediate payload crosses no region" d-immediate)
(bounded "control: an io park with no boundary reclaims normally" d-none)
(bounded "control: a squelched body that never violates reclaims normally"
         d-clean)
(bounded "control: a raise at a boundary is not a park" d-raise)

(bounded "an io park's request is released by the boundary that ends it" d-io)
(bounded "a portless io park owes the same two references" d-sleep)
(bounded "a squelch of :io ends the park exactly as an attune does" d-squelch-io)
(bounded "an emit park's delivery retain has no reader at a boundary" d-emit)
(bounded "the emit primitive's park owes the same delivery" d-dynamic)

(println "region-boundary-park: ok")
