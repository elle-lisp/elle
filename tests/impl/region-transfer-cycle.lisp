(elle/epoch 13)
# audited: 2026-09-29
# A cycle a producer returns across the return or fiber frontier stays whole for a reading consumer and frees nothing live.
# docs/impl/region/owner.md
#
# A producer hands an a<->b cycle across the return (or fiber-terminal)
# frontier, and the consumer discards or reads it. The transfer cut makes the
# consuming activation the cycle's owner. The `returned-cycle` probe in
# tests/impl/probe/direct.lisp pins the reclaim rate; this file pins value
# correctness. The sidecar arms guardfree, so a release that frees a member a
# live frame still holds faults at the read.

(defn cyc-mk []
  (let [a @[]
        b @[]]
    (%array-push a b)
    (%array-push b a)
    a))

# Discarded consumers, repeated — the transfer shape proper.
(def @n 0)
(while (%lt n 20)
  (begin
    (cyc-mk)
    nil)
  (assign n (%add n 1)))

# A READ consumer. The cut's discard gate refuses it, so the cycle stays on
# per-region reference counting, and the returned root still holds exactly its
# cycle partner.
(defn cyc-rd []
  (let [a @[]
        b @[]]
    (%array-push a b)
    (%array-push b a)
    a))
(assert (= (length (cyc-rd)) 1) "returned cycle root holds its one member")

# The fiber-terminal face: a silent body returns the cycle to a discarding
# resume, then the consumer completes.
(defn run-fiber []
  (let [f (fiber/new (fn [] (cyc-mk)) 1)]
    (begin
      (fiber/resume f)
      nil)))
(def @k 0)
(while (%lt k 10)
  (run-fiber)
  (assign k (%add k 1)))

# A parked-then-cancelled consumer fiber calling the producer — the teardown
# face: the kill frees whatever the parked activation owned.
(defn run-cancel []
  (let [f (fiber/new (fn []
                       (begin
                         (cyc-mk)
                         (emit :yield 0)
                         (cyc-mk)
                         nil)) 2)]
    (begin
      (fiber/resume f)
      (fiber/cancel f :dead)
      (fiber/status f))))
(assert (= (run-cancel) :dead) "a cancelled consumer fiber reads :dead")
