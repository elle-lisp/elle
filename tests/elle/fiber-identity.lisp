(elle/epoch 11)
## Fiber identity and status transitions
##
## Migrated from src/value/fiber.rs Rust tests.

# fiber_status_transitions: new -> paused -> dead
(let [f (fiber/new (fn []
                     (yield 42)
                     99) |:yield|)]
  (assert (= (string (fiber/status f)) "new") "fiber starts in new status")
  (fiber/resume f)
  (assert (= (string (fiber/status f)) "paused") "fiber is paused after yield")
  (fiber/resume f)
  (assert (= (string (fiber/status f)) "dead") "fiber is dead after return"))

# fiber_status_error: error signal sets error status
(let [f (fiber/new (fn [] (error "boom")) ||)]
  (protect ((fn [] (fiber/resume f))))
  (assert (= (string (fiber/status f)) "error")
          "fiber is in error status after unhandled error"))

# fiber_values_sharing_a_handle_are_identity_equal
# Two references to the same fiber must compare equal.
(let [f (fiber/new (fn []
                     (yield 1)
                     2) |:yield|)]
  (let [a f
        b f]
    (assert (= a b) "two refs to the same fiber are equal")))

# distinct fibers are not equal
(let [f1 (fiber/new (fn [] 1) ||)
      f2 (fiber/new (fn [] 2) ||)]
  (assert (not (= f1 f2)) "distinct fibers are not equal"))

(println "fiber-identity: all tests passed")
