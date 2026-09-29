(elle/epoch 13)
# audited: 2026-09-29
# This implementation charges no fuel at a forward jump, so one unit of fuel runs an if to completion.
# docs/impl/vm.md
#
# The interpreter charges fuel at backward jumps and at calls. An `if`
# compiles to a JumpIfFalse and a forward Jump, and neither charges. The charge
# sites are this implementation's choice: another implementation may charge
# elsewhere and still pause a runaway fiber.
#
# The counter-factual: a forward jump that charged fuel leaves the fiber
# :paused rather than :dead.

(let [f (fiber/new (fn [] (if true 42 0)) |:fuel|)]
  (fiber/set-fuel f 1)
  (fiber/resume f)
  (assert (= (fiber/status f) :dead) "forward-jump: fiber completes with fuel=1")
  (assert (= (fiber/value f) 42) "forward-jump: correct return value"))
