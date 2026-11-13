(elle/epoch 16)
# audited: 2026-11-13
# A fiber `ev/spawn` creates in a sandbox denied :gpu is refused its SPIR-V compile, not resumed with nil.
# docs/signals/capabilities.md
#
# tests/lang/caps-spawn.lisp pins the same refusal for each capability every
# implementation carries, and shares its sandbox with this file. `git`, the
# call :gpu gates here, compiles a closure to SPIR-V, an extension of this
# implementation. The gate runs before the compile, so no GPU need exist.
#
# The counter-factual: a spawned fiber whose mask does not name :gpu lets the
# denial propagate past the scheduler, and the sandbox's :error-only mask then
# fails the file at the root.

(include-file "../lang/caps/spawn.lisp")

(assert (refused? (spawned |:gpu| (fn [] (protect (git (fn [x] x))))) "git" :gpu)
        "a denied SPIR-V compile is refused")
