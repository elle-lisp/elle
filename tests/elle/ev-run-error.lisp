## tests/elle/ev-run-error.lisp — ev/run must propagate fiber errors
##
## Before the fix, ev/run swallowed errors from fibers: the process
## exited with code 0, no error message.

# Test 1: protect captures error
(let [[[ok? val] (protect (error {:error :test-error :message "boom"}))]]
  (assert (not ok?) "protect should capture error")
  (assert (= (get val :error) :test-error) "error kind preserved"))
(println "  basic protect: ok")

# Test 2: ev/spawn error via ev/join-protected
(let [[f (ev/spawn (fn [] (error {:error :spawn-error :message "inner boom"})))]]
  (let [[[ok? val] (ev/join-protected f)]]
    (assert (not ok?) "ev/join-protected should return false for errored fiber")
    (assert (= (get val :error) :spawn-error) "error kind from spawn")))
(println "  ev/spawn error propagation: ok")

# Test 3: unhandled ev/spawn error propagates through ev/join
(let [[[ok? val] (protect
                   (let [[f (ev/spawn (fn [] (error {:error :unhandled :message "crash"})))]]
                     (ev/join f)))]]
  (assert (not ok?) "ev/join should propagate unhandled error")
  (assert (= (get val :error) :unhandled) "unhandled error kind preserved"))
(println "  ev/join error propagation: ok")

(println "ev-run-error: all tests passed")
