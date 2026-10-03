(elle/epoch 13)
# audited: 2026-09-29
# This implementation's depth cap starts at 10,000,000 calls, and vm/config-set changes it or raises.
# docs/impl/vm.md
#
# The number is this implementation's constant, chosen so a runaway recursion
# stops after a few gigabytes. Another implementation may bound depth
# differently, or not at all.

(def depth 10000)

(defn sum-to [n]
  (if (= n 0)
    0
    (+ n (sum-to (- n 1)))))

# Run `thunk` and answer the error kind it raised, or nil when it returned.
(defn raised [thunk]
  (let [[ok? err] (protect (thunk))]
    (if ok? nil (get err :error))))

(assert (= (vm/config :max-depth) 10000000) "the default depth cap")
(assert (= (get (vm/config) :max-depth) 10000000)
        "the full config struct carries the depth cap")
(vm/config-set :max-depth 20000)
(assert (= (vm/config :max-depth) 20000) "vm/config-set changes the depth cap")
(assert (= (sum-to depth) 50005000) "a recursion under the cap completes")
(assert (= (raised (fn [] (vm/config-set :max-depth 0))) :argument-error)
        "a zero depth cap is refused")
(assert (= (raised (fn [] (vm/config-set :max-depth :deep))) :type-error)
        "a depth cap that is not an integer is refused")
(assert (= (vm/config :max-depth) 20000) "a refused cap changes nothing")
(vm/config-set :max-depth 10000000)
