(elle/epoch 18)
## audited: 2026-11-22
## A subprocess a worker thread spawns starts with no signal blocked, whatever the worker blocks.
## tests/cli/overview.md
## docs/subprocess.md
##
## Elle blocks every signal on each thread it spawns, so its signalfd machinery
## owns delivery. `fork(2)` copies the spawning thread's mask into the child,
## and `execve(2)` keeps it, so a child of such a worker would start with every
## maskable signal blocked: a SIGTERM would queue and never land, and the wait
## would hang. `subprocess/exec` resets the child's mask. The child reports its
## own mask from /proc, so the check needs Linux.
##
## The counter-factual: before the reset, the child's SigBlk is the full
## maskable set, not 0000000000000000.

(include-file "../runner/lib/harness.lisp")

(gate! (file-exists? "/proc/self/status") "no /proc to read a mask from" nil)

(with-temp-dir dir
  (eprintln "scenario: a worker-spawned child has an empty signal mask")
  # The spawn runs inside an os/spawn worker and its own ev/run, as the test
  # runner runs a whole-file script.
  (let [r (elle-run [(write-fixture dir "mask.lisp"
                                    (string
                                      "(println (os/join (os/spawn (fn []\n"
                                      "  (ev/run (fn []\n"
                                      "    (let [p (subprocess/exec \"sh\" [\"-c\" \"grep SigBlk /proc/self/status\"])\n"
                                      "          s (string (port/read-all (get p :stdout)))]\n"
                                      "      (subprocess/wait p)\n"
                                      "      s)))))))\n"))])]
    (assert (= r:exit 0) (string "the run failed: " r:exit "; stderr: " r:err))
    (assert (string/contains? r:out "0000000000000000")
            (string "the worker-spawned child inherited a blocked mask: "
                    (string/trim r:out)))))

(eprintln "all subprocess-sigmask scenarios passed")
