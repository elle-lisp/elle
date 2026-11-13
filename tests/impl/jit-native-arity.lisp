(elle/epoch 15)
# audited: 2026-11-11
# Compiled code raises a native's arity error at every call the compiler did not prove, as the interpreter does.
# src/jit/AGENTS.md
#
# The compiler proves a native's argument count only where the callee is the
# primitive's own binding and the arguments are written out. A call through a
# parameter, and every spliced call, carries no proof, so the native's arity is
# checked when the call runs. `arena/count` takes no argument and reads none,
# so a call that skips the check answers a count instead of the error.
#
# Each face calls a compiled function in call position, so the function's
# compiled code makes the native call: in call position or in tail position,
# through a parameter or to the primitive's binding, spliced or not.
#
# The sidecar compiles each function on the VM thread at its first call, and
# turns the graft off, which would inline the parameter as the primitive.

(defn via-call [f x]
  (let [r (f x)]
    r))
(defn via-tail [f x]
  (f x))
(defn via-splice [f xs]
  (let [r (f ;xs)]
    r))
(defn via-splice-tail [f xs]
  (f ;xs))
(defn direct-splice [xs]
  (let [r (arena/count ;xs)]
    r))
(defn direct-splice-tail [xs]
  (arena/count ;xs))

(defn raised [thunk]
  "Run `thunk` twice, so the second run is compiled, and answer the second
  run's error kind, or :value where it answered a value."
  (protect (thunk))
  (let [[ok? v] (protect (thunk))]
    (if ok? :value (get v :error))))

(defn at2 [g a b]
  (let [r (g a b)]
    r))
(defn at1 [g a]
  (let [r (g a)]
    r))

(assert (= :arity-error (raised (fn [] (at2 via-call arena/count 1))))
        "a call through a parameter")
(assert (= :arity-error (raised (fn [] (at2 via-tail arena/count 1))))
        "a tail call through a parameter")
(assert (= :arity-error (raised (fn [] (at2 via-splice arena/count [1]))))
        "a spliced call through a parameter")
(assert (= :arity-error (raised (fn [] (at2 via-splice-tail arena/count [1]))))
        "a spliced tail call through a parameter")
(assert (= :arity-error (raised (fn [] (at1 direct-splice [1]))))
        "a spliced call to the primitive's binding")
(assert (= :arity-error (raised (fn [] (at1 direct-splice-tail [1]))))
        "a spliced tail call to the primitive's binding")

(let [[ok? v] (protect (at1 direct-splice [1]))]
  (assert (= (get v :message) "debug/arena-count: expected 0 argument(s), got 1")
          "the error names the native and both counts, as the interpreter's does"))

(when (vm/config :jit)
  (assert (all? jit?
                [via-call via-tail via-splice via-splice-tail direct-splice
                 direct-splice-tail]) "precondition: every caller compiles"))

(println "jit-native-arity: ok")
