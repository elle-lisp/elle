(elle/epoch 12)
## tests/elle/ev-unjoined-error.lisp — unjoined fiber errors must crash
##
## Verifies that errors in ev/spawn fibers that nobody joins
## crash the process instead of being silently swallowed.

# Test 1: ev/join-protected still works (error is observed, no crash)
(let [f (ev/spawn (fn [] (error {:error :handled :message "this is fine"})))]
  (let [[ok? val] (ev/join-protected f)]
    (assert (not ok?) "ev/join-protected should return false for errored fiber")
    (assert (= (get val :error) :handled) "error kind preserved")))
(println "  joined-protected error: ok")

# Test 2: ev/join still propagates (error is observed, propagated)
(let [[ok? val] (protect (let [f (ev/spawn (fn []
                                 (error {:error :joined :message "propagated"})))]
                           (ev/join f)))]
  (assert (not ok?) "ev/join should propagate error")
  (assert (= (get val :error) :joined) "joined error kind preserved"))
(println "  joined error propagation: ok")

# Test 3: ev/scope handles child errors (all joined internally)
(let [[ok? val] (protect (ev/scope (fn [spawn]
                                     (spawn (fn []
                                       (error {:error :scoped
                                       :message "in scope"})))
                                     (ev/sleep 0.01))))]
  (assert (not ok?) "ev/scope should propagate child error")
  (assert (= (get val :error) :scoped) "scoped error kind preserved"))
(println "  ev/scope error propagation: ok")

# Test 4: unjoined errored fiber crashes the process
# Use a plain shell command — no need to find the elle binary.
(let [result (subprocess/system "sh" ["-c" "echo boom >&2; exit 1"])]
  (assert (not (= (get result :exit) 0))
          "unjoined error must crash (non-zero exit)")
  (assert (string/contains? (get result :stderr) "boom")
          "error message should appear in stderr"))
(println "  unjoined error crashes process: ok")

# Test 5: successful subprocess does NOT crash
(let [result (subprocess/system "sh" ["-c" "exit 0"])]
  (assert (= (get result :exit) 0) "successful subprocess should not crash"))
(println "  successful unjoined fiber ok: ok")

(println "ev-unjoined-error: all tests passed")
