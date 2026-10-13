(elle/epoch 15)
# audited: 2026-10-13
# An ffi/ primitive that runs C code or touches C memory requires :ffi; one that only describes a type does not.
# docs/ffi.md
#
# The counter-factual is ffi/run-teardowns signalling only :error. It calls
# each declared teardown's C symbol, yet a fiber under :deny |:ffi| runs it to
# completion and returns nil instead of pausing on a capability denial.

(defn denied? [thunk]
  (let [f (fiber/new thunk |:ffi :error| :deny |:ffi|)]
    (fiber/resume f)
    (and (= (fiber/status f) :paused)
         (= (get (fiber/value f) :error) :capability-denied)
         ((get (fiber/value f) :denied) :ffi))))

(assert (denied? (fn [] (ffi/run-teardowns)))
        "ffi/run-teardowns is denied under :deny |:ffi|")
(assert (denied? (fn [] (ffi/malloc 8)))
        "ffi/malloc is denied under :deny |:ffi|")
(assert (denied? (fn [] (ffi/native nil)))
        "ffi/native is denied under :deny |:ffi|")

(let [f (fiber/new (fn [] (ffi/size :i32)) |:ffi :error| :deny |:ffi|)]
  (assert (= (fiber/resume f) 4)
          "ffi/size describes a type, so :ffi is not needed"))
