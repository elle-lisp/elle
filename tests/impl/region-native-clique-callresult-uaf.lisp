(elle/epoch 13)
# audited: 2026-09-28
# A call-result argument installed in another fiber's signal slot survives
# that fiber.
#
# `[:k :reason]` lowers to an array-constructor call, so its region is a
# call-result region that no alloc opcode of this body names. `fiber/cancel`
# and `fiber/abort` of a :new fiber each kill it, park the argument as its
# terminal signal, and return the argument. Both declare
# `Delivers { args: &[1] }` (docs/impl/region/effects.md), so the kill counts
# the install itself: the park-retain and the recorded `fiber → signal` edge,
# which the fiber's free balances.
#
# The counter-factual is an install nothing counts. When f's region frees at
# f's last use, the free cascade finds the signal's array and decrefs its
# region, freeing v under its reader. The fiber/status read between the install
# and v's read allocates into the freed page, so the stale read shows
# deterministically.
#
# docs/impl/region/clique.md holds the earlier shape of the same fault: a
# native with an uncounted-store effect, whose slot-based incref of a
# call-result argument resolves to nothing.

(let* [f (fiber/new (fn [] "never-reached") 1)
       v (fiber/cancel f [:k :reason])
       s (fiber/status f)]
  (assert (= s :dead) "cancel: fiber status is dead")
  (assert (= v [:k :reason]) "cancel: stored+returned arg survives the fiber"))

(let* [f (fiber/new (fn [] "never-reached") 1)
       v (fiber/abort f [:k :reason])
       s (fiber/status f)]
  (assert (= s :error) "abort: fiber status is error")
  (assert (= v [:k :reason]) "abort: stored+returned arg survives the fiber"))

(println "region-native-clique-callresult-uaf: ok")
