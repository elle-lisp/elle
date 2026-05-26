(elle/epoch 11)
## tests/elle/prim-subprocess.lisp
## Exit code validation, subprocess/exec, sys/args, sys/env

## ── exit validation ────────────────────────────────────────────────

# compile-time arity enforcement is tested implicitly;
# these tests verify runtime argument validation via eval
(let [[ok? _] (protect ((fn [] (eval (read "(sys/exit 0 1)")))))]
  (assert (not ok?) "exit too many args"))
(let [[ok? _] (protect ((fn [] (eval (read "(sys/exit true)")))))]
  (assert (not ok?) "exit wrong type"))
(let [[ok? _] (protect ((fn [] (eval (read "(sys/exit -1)")))))]
  (assert (not ok?) "exit negative"))
(let [[ok? _] (protect ((fn [] (eval (read "(sys/exit 256)")))))]
  (assert (not ok?) "exit too large"))

## ── sys/env ────────────────────────────────────────────────────────

(let [env (sys/env)]
  (assert (struct? env) "sys/env returns struct"))

(assert (string? (sys/env "PATH")) "sys/env PATH is string")
(assert (nil? (sys/env "DEFINITELY_NOT_SET_XYZ_ELLE_123"))
        "sys/env unset returns nil")

(let [[ok? _] (protect ((fn [] (sys/env 42))))]
  (assert (not ok?) "sys/env non-string type error"))

## ── subprocess/exec ────────────────────────────────────────────────

(let [[ok? _] (protect ((fn [] (subprocess/exec 42 @[]))))]
  (assert (not ok?) "subprocess/exec program not string"))
(let [[ok? _] (protect ((fn [] (subprocess/exec "echo" "not-array"))))]
  (assert (not ok?) "subprocess/exec args not array"))
(let [[ok? _] (protect ((fn [] (subprocess/exec "echo" @[99]))))]
  (assert (not ok?) "subprocess/exec args element not string"))

## ── subprocess/wait ────────────────────────────────────────────────

(let [[ok? _] (protect ((fn [] (subprocess/wait 42))))]
  (assert (not ok?) "subprocess/wait wrong type"))

## ── subprocess/kill ────────────────────────────────────────────────

(let [[ok? _] (protect ((fn [] (eval (read "(subprocess/kill)")))))]
  (assert (not ok?) "subprocess/kill arity 0"))
(let [[ok? _] (protect ((fn [] (subprocess/kill 42))))]
  (assert (not ok?) "subprocess/kill wrong type"))

## ── subprocess/pid ─────────────────────────────────────────────────

(let [[ok? _] (protect ((fn [] (subprocess/pid 42))))]
  (assert (not ok?) "subprocess/pid wrong type"))

(println "prim-subprocess: all tests passed")
