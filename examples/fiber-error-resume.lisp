#!/usr/bin/env elle

# Fiber error resume — what happens when you resume a terminal fiber
#
# Demonstrates:
#   fiber/new with mask=0    — errors propagate to parent, fiber enters :error
#   Resuming :error fiber    — fiber/resume rejects with "cannot resume errored fiber"
#   fiber/new with mask=1    — parent catches errors, fiber stays :suspended
#   fiber/cancel             — injects error, fiber enters :error (terminal)
#   Terminal vs resumable    — the mask determines whether an error is terminal

(import-file "./examples/assertions.lisp")


# A fiber with mask=0 does not catch errors. When the child errors,
# the error propagates to the parent. The child enters :error status.

(def f0 (fiber/new (fn [] (error [:boom "kaboom"])) 0))

# The error propagates to us, so we must catch it to survive.
(def [ok? err] (protect (fiber/resume f0)))
(display "  mask=0 fiber errored: ") (print err)
(assert-false ok? "mask=0: error propagates to parent")
(assert-eq (get err 0) :boom "mask=0: error kind preserved")

(def status0 (fiber/status f0))
(display "  fiber status: ") (print status0)
(assert-eq status0 :error "mask=0: fiber is in :error status")


# Now try to resume the errored fiber. This should fail.

(def [ok2? err2] (protect (fiber/resume f0)))
(display "  resume errored fiber: ") (print err2)
(assert-false ok2? "resume errored: raises an error")
(assert-eq (get err2 0) :error "resume errored: error kind is :error")

# The fiber is still in :error — nothing changed.
(assert-eq (fiber/status f0) :error "fiber still :error after failed resume")


# A fiber with mask=1 catches errors. The parent sees the error as
# a caught signal — the child stays :suspended, not :error.

(def f1 (fiber/new (fn [] (error [:caught-boom "handled"])) 1))
(fiber/resume f1 nil)

(def status1 (fiber/status f1))
(def value1 (fiber/value f1))
(display "  mask=1 fiber status: ") (print status1)
(display "  mask=1 fiber value: ") (print value1)
(assert-eq status1 :suspended "mask=1: fiber is :suspended, not :error")
(assert-eq (get value1 0) :caught-boom "mask=1: error value accessible")

# A suspended fiber can be resumed (though it has nothing left to do).
(fiber/resume f1 nil)
(def status1b (fiber/status f1))
(display "  mask=1 after second resume: ") (print status1b)
(assert-eq status1b :dead "mask=1: fiber completes on second resume")


# fiber/cancel injects an error into a suspended fiber, making it
# terminal (:error) regardless of its mask.

(def f2 (fiber/new (fn [] (yield :waiting) :done) 3))
(fiber/resume f2 nil)
(assert-eq (fiber/status f2) :suspended "cancel target: starts suspended")
(assert-eq (fiber/value f2) :waiting "cancel target: yielded value")

(fiber/cancel f2 [:cancelled "externally cancelled"])
(def status2 (fiber/status f2))
(display "  cancelled fiber status: ") (print status2)
(assert-eq status2 :error "cancel: fiber is :error")

# Resuming a cancelled fiber fails the same way as any errored fiber.
(def [ok3? err3] (protect (fiber/resume f2)))
(display "  resume cancelled fiber: ") (print err3)
(assert-false ok3? "resume cancelled: raises an error")
(assert-eq (get err3 0) :error "resume cancelled: error kind is :error")

(print "")
(print "all fiber-error-resume passed.")
