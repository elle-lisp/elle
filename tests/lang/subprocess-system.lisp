(elle/epoch 15)
# audited: 2026-11-17
# subprocess/system finishes a child that fills one pipe while the other is still open.
# docs/subprocess.md
#
# The trap: a child blocks when it writes to a full pipe, and a pipe holds
# about 64 KB. A caller that reads stdout to its end before it reads stderr
# never sees stdout end, because the child is stuck writing stderr. Each call
# here floods 1 MB, past any pipe's buffer. The counter-factual is a call that
# never answers, so ev/timeout turns it into a failed assertion; a correct call
# takes milliseconds.

(def flood "head -c 1048576 /dev/zero")

(defn system-or-nil [script]
  "What subprocess/system answers for SCRIPT under sh, or nil when it is still
   waiting after 20 seconds."
  (ev/timeout 20 (fn [] (subprocess/system "sh" ["-c" script]))))

(let [r (system-or-nil (string flood " >&2; echo done"))]
  (assert (not (nil? r))
          "a child that floods stderr before stdout ends finishes")
  (assert (= (get r :exit) 0) "and exits cleanly")
  (assert (= (get r :stdout) "done\n") "stdout holds what followed the flood")
  (assert (= (string/size-of (get r :stderr)) 1048576)
          "stderr holds every byte of the flood"))

(let [r (system-or-nil (string flood "; " flood " >&2; echo done"))]
  (assert (not (nil? r))
          "a child that floods stdout and then stderr before stdout ends finishes")
  (assert (= (get r :exit) 0) "and exits cleanly")
  (assert (= (string/size-of (get r :stdout)) (+ 1048576 5))
          "stdout holds its flood and the line after it")
  (assert (= (string/size-of (get r :stderr)) 1048576)
          "stderr holds every byte of its flood"))
