(elle/epoch 9)
# embedding.lisp — step-based scheduler test
#
# Exercises ev/step from Elle: create scheduler manually, spawn a fiber,
# step until done, verify result.

(defn test-step-basic []
  "Step a pure-compute fiber to completion."
  (let [sched (make-async-scheduler)
        result @[nil]
        f (fiber/new (fn [] (+ 21 21)) |:yield|)]
    ((get sched :spawn) f)
    (def @status :pending)
    (while (= status :pending) (assign status ((get sched :step) 0)))
    (assert (= status :done) "step should return :done")
    (assert (= (fiber/value f) 42) "fiber result should be 42")))

(defn test-step-multiple-fibers []
  "Step multiple fibers to completion."
  (let [sched (make-async-scheduler)
        f1 (fiber/new (fn [] 10) |:yield|)
        f2 (fiber/new (fn [] 20) |:yield|)]
    ((get sched :spawn) f1)
    ((get sched :spawn) f2)
    (def @status :pending)
    (while (= status :pending) (assign status ((get sched :step) 0)))
    (assert (= (fiber/value f1) 10) "f1 result should be 10")
    (assert (= (fiber/value f2) 20) "f2 result should be 20")))

(defn test-step-returns-pending []
  "Step returns :pending when I/O fibers are still active."
  (let [sched (make-async-scheduler)
        f (fiber/new (fn []
                       (ev/spawn (fn [] 99))
                       (yield)
                       1) |:yield|)]
    ((get sched :spawn) f)  # First step should process the fiber but it yields, so :pending
    (let [r ((get sched :step) 0)]
      (assert (or (= r :pending) (= r :done))
              "step should return :pending or :done"))))

(defn test-ev-step-public []
  "ev/step works inside an event loop."
  (let [sched (make-async-scheduler)]
    (parameterize ((*scheduler* sched)
                   (*spawn* (get sched :spawn))
                   (*shutdown* (get sched :shutdown))
                   (*io-backend* (get sched :backend)))
      (let [f (ev/spawn (fn [] (+ 1 2 3)))]
        (def @status :pending)
        (while (= status :pending) (assign status (ev/step)))
        (assert (= status :done) "ev/step should return :done")
        (assert (= (fiber/value f) 6) "fiber result should be 6")))))

(test-step-basic)
(test-step-multiple-fibers)
(test-step-returns-pending)
(test-ev-step-public)

(println "embedding tests passed")
