(elle/epoch 12)
## The transferred-returned-cycle shapes (docs/impl/region/owner.md § "Owner
## nodes" — "The transferred returned subtree") run soundly on the default
## baseline: a producer hands an a<->b cycle across the return (or
## fiber-terminal) frontier and the consumer discards or reads it. On the
## flag-off baseline the discarded cycles leak (the oracle's `returned-cycle`
## probe pins the rate); this file pins VALUE correctness and is the
## guardfree subject for the shapes on both tiers.
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

# A READ consumer (refused by the cut's discard gate; must stay correct on
# the RC baseline): the returned root holds exactly its cycle partner.
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
# face (the kill frees whatever the parked activation owned, flag-on).
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
(assert (= (run-cancel) :error) "a cancelled consumer fiber reads :error")
