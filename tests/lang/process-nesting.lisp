(elle/epoch 12)
# audited: 2026-09-23
# Nested schedulers: a process scheduler inside ev/run or inside a process does its I/O through its parent.
# docs/process-scheduler.md

(def process ((import "std/process")))

(defn inner-work []
  "Work for a nested scheduler's PID 0: a timer, a sleep and a subprocess.
   Returns the subprocess's output."
  (process:recv-timeout 3)
  (ev/sleep 0.001)
  (get (subprocess/system "echo" ["nested"]) :stdout))

# ── inside ev/run ────────────────────────────────────────────────────
# ev/run builds an async scheduler with its own backend, so the process
# scheduler's parent here is not the root.

(ev/run (fn []
          (let [out @[]]
            (process:start (fn [] (push out (inner-work))))
            (assert (= (get out 0) "nested\n")
                    "a process scheduler inside ev/run does its I/O"))))

# ── inside a process ─────────────────────────────────────────────────
# The counter-factual: the outer process scheduler knew no :io-forward
# wait op, so the inner scheduler's first I/O raised :protocol-error.

(process:start (fn []
                 (let [me (process:self)]
                   (process:spawn (fn []
                                    (let [out @[]]
                                      (process:start (fn []
                                        (push out (inner-work))))
                                      (process:send me [:inner (get out 0)]))))
                   (assert (= (process:recv-timeout 10000) [:inner "nested\n"])
                           "a process scheduler inside a process does its I/O"))))

# Three schedulers deep, each request crosses both outer schedulers.
(process:start (fn []
                 (let [me (process:self)]
                   (process:spawn (fn []
                                    (let [out @[]]
                                      (process:start (fn []
                                        (process:start (fn []
                                          (push out (inner-work))))))
                                      (process:send me [:deepest (get out 0)]))))
                   (assert (= (process:recv-timeout 10000) [:deepest "nested\n"])
                           "a scheduler two levels down does its I/O"))))

# A sub-fiber of a process can run the nested scheduler too.
(process:start (fn []
                 (let [out @[]]
                   (ev/join (ev/spawn (fn []
                                        (process:start (fn []
                                          (push out (inner-work)))))))
                   (assert (= (get out 0) "nested\n")
                           "a process scheduler inside a sub-fiber does its I/O"))))

# Several processes run nested schedulers at once, and each gets its own
# completions.
(process:start (fn []
                 (let [me (process:self)]
                   (each tag in [:a :b :c]
                     (process:spawn (fn []
                                      (let [out @[]]
                                        (process:start (fn []
                                          (ev/sleep 0.001)
                                          (push out
                                          (get (subprocess/system "echo"
                                          [(string tag)]) :stdout))))
                                        (process:send me [tag (get out 0)])))))
                   (def got @{})
                   (repeat 3
                           (match (process:recv-timeout 10000)
                             [tag text] (put got tag text)
                             other (assert false
                             (string "expected a nested result, got " other))))
                   (assert (= (get got :a) "a\n")
                           "the first nest got its own output")
                   (assert (= (get got :b) "b\n")
                           "the second nest got its own output")
                   (assert (= (get got :c) "c\n")
                           "the third nest got its own output"))))

# ── a process that exits with relayed I/O in flight ──────────────────
# The inner scheduler sleeps for a minute. Killing the process that runs it
# cancels the relayed sleep. The counter-factual for the cancel: the relayed
# entry outlived its process, and the outer scheduler waited out the whole
# minute before it returned.

(let [done (ev/timeout 20
                       (fn []
                         (process:start (fn []
                                          (let* [asleep (box false)
                                            runner (process:spawn (fn []
                                              (process:start (fn []
                                                (rebox asleep true)
                                                (ev/sleep 60)))))
                                            ref (process:monitor runner)]
                                            (while (not (unbox asleep))
                                              (process:recv-timeout 1))
                                            (process:recv-timeout 2)
                                            (process:exit runner :kill)
                                            (assert (= (process:recv-timeout 100)
                                            [:DOWN ref runner [:killed :kill]])
                                            "the runner dies of the kill, in its sleep"))))
                         :returned))]
  (assert (= done :returned)
          "the outer scheduler returns without waiting out the relayed sleep"))

(println "process-nesting: ok")
