(elle/epoch 12)
## tests/elle/stdin-close.lisp — verify (port/close *stdin*) cancels
## an in-flight read and the child program exits cleanly.
##
## We spawn an elle child with `:stdin :pipe` and never write to that
## pipe. Inside the child, a fiber blocks on `(port/read-line (*stdin*))`.
## After a brief sleep the child calls `(port/close (*stdin*))`. The fix
## (`src/io/threadpool.rs::StdinThread::shutdown`) wakes the worker
## thread out of its blocking `libc::read(0, …)`, surfaces a
## `stdin closed` error to the fiber, and lets the child reach
## `(sys/exit 0)`.
##
## Failure mode without the fix: the worker thread sits in
## `std::io::stdin().lock().read_line(…)` (auto-retries EINTR, no
## shutdown path), the fiber never resumes, the child hangs at
## `ev/join`, the parent's `subprocess/wait` hangs in turn, and the
## outer 30 s smoke-test timeout `SIGTERM`s us. With the fix wired up,
## the child exits within ~200 ms.

(def elle
  (or (get (sys/env) "ELLE")
      (if (file-exists? "./target/release/elle")
        "./target/release/elle"
        "./target/debug/elle")))

(def child-file (string "/tmp/elle-stdin-close-child-" (sys/pid) ".lisp"))

(def child-code
  "## Spawn a fiber that blocks on stdin, close *stdin* from the main
   ## fiber, observe the spawned fiber resume (with an error from the
   ## cancelled read), and exit cleanly. If close-stdin doesn't wake
   ## the worker thread, this child hangs forever and the parent
   ## test's subprocess/system never returns.
   (def f (ev/spawn (fn [] (port/read-line (*stdin*)))))
   (ev/sleep 0.2)
   (eprintln \"child: closing stdin\")
   (port/close (*stdin*))
   (eprintln \"child: joining fiber\")
   (let [[ok? val] (protect ((fn [] (ev/join f))))]
     (eprintln \"child: result ok?=\" ok? \" val=\" val))
   (eprintln \"child: exiting\")
   (sys/exit 0)")

(file/write child-file child-code)

(defer
  (file/delete child-file)
  (def r (subprocess/system elle [child-file] {:stdin :pipe}))
  (eprintln "parent: child exit=" (get r :exit))
  (eprintln "parent: child stderr=" (get r :stderr))
  (assert (= 0 (get r :exit))
          (string "child must exit 0 after close-stdin; got exit=" (get r :exit)))
  (assert (string/contains? (or (get r :stderr) "") "child: exiting")
          (string "child must reach the 'exiting' line; stderr=" (get r :stderr))))

(println "stdin-close: all tests passed")
