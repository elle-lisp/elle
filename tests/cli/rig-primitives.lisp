(elle/epoch 18)
## audited: 2026-11-24
## A user build carries none of the primitives the rig registers, and its refusal names the one called.
## tests/cli/overview.md
## rig/overview.md
##
## The rig registers `elle/build`, `elle/tokens` and `jit/stats` on its own
## runtimes. `jit/stats` polls the compile worker, and a user build carries no
## poll. The counter-factual: a primitive registered in the library rather than
## the rig answers under `elle` too. The check lives here, not beside the rig's
## own tests in tests/impl, because the MLIR job builds a rig and no `elle`.

(include-file "../runner/lib/harness.lisp")

(eprintln "scenario: elle refuses each of the rig's primitives by name")
(each [name call] [["elle/build" "(elle/build)"]
                   ["elle/tokens" "(elle/tokens \"a\")"]
                   ["jit/stats" "(jit/stats)"]]
  (let [r (elle-run ["-e" call])]
    (assert (not (= r:exit 0))
            (string "a user build must not answer " call "; stdout: " r:out))
    (assert (string/contains? r:err name)
            (string "the refusal names " name ": " r:err))))

(eprintln "all rig-primitives scenarios passed")
