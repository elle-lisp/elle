(elle/epoch 12)
## tests/elle/ev-run-error-teardown.lisp
##
## An `ev/run` whose body fails reports the failure, whatever else is still
## parked.
##
## `ev/run` tears its scheduler down as soon as the program's own fibers
## finish, aborting every fiber the program never awaited — a sleeper, a futex
## waiter, a reader on a socket nobody closed. That teardown has to happen when
## the program *fails* exactly as it does when the program returns, or a
## failing program hangs instead of reporting, and the failure becomes a
## deadline rather than an error.
##
## The trap is that "the program finished" is not a status comparison. A fiber
## that stopped on an uncaught error reports `:paused` — the keyword a fiber
## waiting to resume also reports — and carries SIG_ERROR in `fiber/bits`
## (docs/signals/primitives.md). Code that asks for `:dead` or `:error` sees a
## failed fiber as still running, waits for it, and waits forever on the first
## orphan that cannot finish by itself.
##
## `fiber/done?` deliberately answers false for that fiber, and case 1 pins
## that: a paused fiber holding an error is one a parent may still resume, and
## the stream generators do exactly that to surface a read error as an element.
## The scheduler cannot ask a predicate over the fiber's status at all — it has
## to read the completion it already recorded.
##
## Case 1 pins the predicates. Cases 2-4 vary what the orphan is parked on,
## because each parks through a different scheduler path: a timer, a park
## queue, an in-flight io op. Case 5 is the control that already worked — the
## same orphans, a program that returns instead of failing.
##
## Every case announces itself first. A regression here is a hang, and the
## marker is what names the case in the runner's problem list.

(defn step [label]
  (eprintln "    · " label))

(defn listen-port [listener]
  "Return the port number of a listener bound to an ephemeral port."
  (let [parts (string/split (port/path listener) ":")]
    (parse-int (get parts (- (length parts) 1)))))

(def boom {:error :boom :message "the program failed"})

## ── 1. The predicates that decide "finished" ─────────────────────────

(step "1: a failed fiber reports itself finished")

(ev/run (fn []
          (let [f (ev/spawn (fn [] (error boom)))]
            (ev/sleep 0.05)
            ## Observe the failure before asserting anything about it. An
            ## unobserved failed fiber crashes the program at teardown, and
            ## that crash would surface here instead of the assertion that
            ## actually fired.
            (protect (ev/join f))
            (assert (= (fiber/status f) :paused)
                    (concat "a failed fiber reports :paused, got "
                            (string (fiber/status f))))
            (assert (not (= 0 (bit/and (fiber/bits f) 1)))
                    "a failed fiber carries SIG_ERROR in its bits")
            ## The status is what a fiber waiting to resume reports, and
            ## `fiber/done?` follows the status: a paused fiber holding an
            ## error is one a parent may still resume. So neither answers
            ## "will this fiber run again" — which is why the scheduler reads
            ## the completion it recorded instead of asking here.
            (assert (not (fiber/done? f))
                    "a resumable failed fiber is not terminal by status")
            ## A fiber that is genuinely still waiting reports the same
            ## status, and carries no error bit. The bit is the whole
            ## difference between the two.
            (let [waiting (ev/spawn (fn [] (ev/sleep 30)))]
              (ev/sleep 0.05)
              (assert (= (fiber/status waiting) (fiber/status f))
                      "a waiting fiber and a failed one report the same status")
              (assert (= 0 (bit/and (fiber/bits waiting) 1))
                      "a waiting fiber carries no error bit")
              (ev/abort waiting)))))

(println "  1. a failed fiber is terminal, and says so")

## ── 2-4. A failing program with an orphan parked on each path ────────

(step "2: fail with an orphan in ev/sleep")

(let [[ok? err] (protect (ev/run (fn []
                                   (ev/spawn (fn [] (ev/sleep 30)))
                                   (ev/sleep 0.05)
                                   (error boom))))]
  (assert (not ok?) "the program's failure must surface")
  (assert (= (get err :error) :boom)
          (concat "expected the program's own error, got " (string err))))

(println "  2. an orphan in ev/sleep does not hold the failure")

(step "3: fail with an orphan parked on a futex")

(let [[ok? err] (protect (ev/run (fn []
                                   (let [key (gensym)
                                     bx (box 0)]
                                     (ev/spawn (fn [] (ev/futex-wait key bx 0)))
                                     (ev/sleep 0.05)
                                     (error boom)))))]
  (assert (not ok?) "the program's failure must surface")
  (assert (= (get err :error) :boom)
          (concat "expected the program's own error, got " (string err))))

(println "  3. an orphan on a futex does not hold the failure")

(step "4: fail with an orphan parked in a read")

(let [[ok? err] (protect (ev/run (fn []
                                   (let* [listener (tcp/listen "127.0.0.1" 0)
                                     lport (listen-port listener)]
                                     (ev/spawn (fn []
                                       (let [peer (tcp/accept listener)]
                                         ## Nothing is ever sent, so
                                         ## this read is the orphan.
                                         (port/read peer 32768))))
                                     (let [conn (tcp/connect "127.0.0.1" lport
                                       :timeout 5000)]
                                       (ev/sleep 0.05)
                                       (error boom))))))]
  (assert (not ok?) "the program's failure must surface")
  (assert (= (get err :error) :boom)
          (concat "expected the program's own error, got " (string err))))

(println "  4. an orphan in a read does not hold the failure")

## ── 5. The control: the same orphans, a program that returns ─────────

(step "5: return with the same orphans")

(assert (= :finished (ev/run (fn []
                               (let* [listener (tcp/listen "127.0.0.1" 0)
                                      lport (listen-port listener)
                                      key (gensym)
                                      bx (box 0)]
                                 (ev/spawn (fn [] (ev/sleep 30)))
                                 (ev/spawn (fn [] (ev/futex-wait key bx 0)))
                                 (ev/spawn (fn []
                                   (let [peer (tcp/accept listener)]
                                     (port/read peer 32768))))
                                 (let [conn (tcp/connect "127.0.0.1" lport
                                       :timeout 5000)]
                                   (ev/sleep 0.05)
                                   :finished)))))
        "a program that returns must not wait for its orphans either")

(println "  5. a returning program abandons the same orphans")

(println "ev-run-error-teardown: a failing program reports, it does not hang")
