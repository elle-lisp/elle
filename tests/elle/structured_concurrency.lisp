(elle/epoch 8)
# Structured concurrency tests
#
# User code runs inside the async scheduler (via execute_scheduled),
# so ev/join, ev/spawn, etc. work directly.

# === 1. ev/join — basic spawn + join ===

(assert (= 42 (ev/join (ev/spawn (fn [] 42))))
        "1a: spawn + join returns value")

# === 2. ev/join — propagates error ===

(let [[ok? val] (protect (ev/join (ev/spawn (fn [] (error {:error :boom})))))]
  (assert (not ok?) "2a: ev/join propagates error")
  (assert (= :boom (get val :error)) "2b: error value preserved"))

# === 3. ev/join — sequence collects ordered results ===

(let [fibers [(ev/spawn (fn [] :a))
               (ev/spawn (fn [] :b))
               (ev/spawn (fn [] :c))]]
  (assert (= [:a :b :c] (ev/join fibers))
          "3a: join sequence collects results in order"))

# === 4. ev/join — double join (two joiners on same target) ===

(let [target (ev/spawn (fn [] 99))
      results @[]]
  (let [a (ev/spawn (fn [] (push results (ev/join target))))
        b (ev/spawn (fn [] (push results (ev/join target))))]
    (ev/join a)
    (ev/join b)
    (assert (= 2 (length results)) "4a: both joiners woken")
    (assert (= 99 (get results 0)) "4b: first joiner got value")
    (assert (= 99 (get results 1)) "4c: second joiner got value")))

# === 5. ev/join-protected — success ===

(let [[ok? val] (ev/join-protected (ev/spawn (fn [] 42)))]
  (assert ok? "5a: ev/join-protected success returns true")
  (assert (= 42 val) "5b: ev/join-protected success returns value"))

# === 6. ev/join-protected — failure ===

(let [[ok? val] (ev/join-protected (ev/spawn (fn [] (error "fail"))))]
  (assert (not ok?) "6a: ev/join-protected failure returns false")
  (assert (= "fail" val) "6b: ev/join-protected returns error value"))

# === 7. ev/abort — on :dead fiber is no-op ===

(let [f (ev/spawn (fn [] 42))]
  (ev/join f)
  (ev/abort f)
  (assert (= :dead (fiber/status f)) "7a: abort on dead fiber is no-op"))

# === 8. fiber/abort — on :new fiber works ===

(let [f (fiber/new (fn [] 42) |:error :io :exec :wait|)]
  (fiber/abort f {:error :aborted})
  (assert (= :error (fiber/status f)) "8a: fiber/abort on :new sets to :error"))

# === 8b. fiber/abort — on :dead fiber is silent no-op (bug #2 Option A) ===
# Before the fix, fiber/abort on a Dead target raised a state-error
# ("cannot abort a completed fiber"). Now it returns SIG_OK with the
# fiber's final value, matching ev/abort's "No-op if already completed"
# docstring. Exercised via (protect ...) so a regression would surface
# as ok?=false.

(let [f (ev/spawn (fn [] 42))]
  (ev/join f)
  (let [[ok? val] (protect (fiber/abort f {:error :aborted}))]
    (assert ok? "8b: fiber/abort on :dead no longer errors")
    (assert (= :dead (fiber/status f)) "8b: :dead fiber stays :dead after abort")))

# === 9. ev/select — returns [completed remaining] ===

(let [fast (ev/spawn (fn [] :fast))
      slow (ev/spawn (fn [] (ev/sleep 10) :slow))]
  (let [[done remaining] (ev/select [fast slow])]
    (assert (= done fast) "9a: select returns fast fiber")
    (assert (= 1 (length remaining)) "9b: one fiber remaining")
    (ev/abort (first remaining))))

# === 10. ev/race — aborts losers ===

(let [fast (ev/spawn (fn [] :fast))
      slow (ev/spawn (fn [] (ev/sleep 10) :slow))]
  (let [result (ev/race [fast slow])]
    (assert (= :fast result) "10a: race returns winner's value")
    (assert (= :error (fiber/status slow)) "10b: loser is aborted")))

# === 11. ev/timeout — doesn't fire ===

(let [result (ev/timeout 10 (fn [] 42))]
  (assert (= 42 result) "11a: timeout returns value when work finishes first"))

# === 12. ev/timeout — fires ===

(let [[ok? val] (protect (ev/timeout 0.01 (fn [] (ev/sleep 100))))]
  (assert (not ok?) "12a: timeout fires when work is slow")
  (assert (= :timeout (get val :error)) "12b: timeout error has :timeout key"))

# === 13. ev/scope — joins all children ===

(let [result (ev/scope (fn [spawn]
                (let [a (spawn (fn [] 1))
                      b (spawn (fn [] 2))]
                  (+ (ev/join a) (ev/join b)))))]
  (assert (= 3 result) "13a: scope joins all children and returns body result"))

# === 14. ev/scope — aborts siblings on error ===

(let [[ok? val] (protect (ev/scope (fn [spawn]
    (spawn (fn [] (ev/sleep 100)))
    (spawn (fn [] (error "child-error")))
    (ev/sleep 100))))]
  (assert (not ok?) "14a: scope propagates first error"))

# === 15. ev/map — returns results in input order ===

(let [results (ev/map (fn [x] (* x x)) [1 2 3 4 5])]
  (assert (= [1 4 9 16 25] results)
          "15a: ev/map returns ordered results"))

# === 16. ev/join — already-dead fiber returns immediately ===

(let [f (ev/spawn (fn [] 99))]
  (ev/join f)
  (assert (= 99 (ev/join f)) "16a: join on already-dead fiber returns value"))

# === 17. ev/join-protected — sequence ===

(let [fibers [(ev/spawn (fn [] 1))
               (ev/spawn (fn [] (error "oops")))
               (ev/spawn (fn [] 3))]]
  (let [results (ev/join-protected fibers)]
    (assert (= [true 1] (get results 0)) "17a: first succeeds")
    (assert (= false (get (get results 1) 0)) "17b: second fails")
    (assert (= [true 3] (get results 2)) "17c: third succeeds")))

(println "All structured concurrency tests passed.")
