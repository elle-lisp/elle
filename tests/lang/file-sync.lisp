(elle/epoch 15)
# audited: 2026-10-21
# file/sync: a file, a directory, a missing path, a fifo, and the :fs capability it needs.
# docs/io/files.md

(defn fails-with [kind thunk]
  "Run thunk, and answer the error it raises after checking its kind."
  (let [[ok? err] (protect (thunk))]
    (assert (not ok?) (string "expected an error of kind " kind))
    (assert (= (get err :error) kind) (string "the kind is " kind ": " err))
    err))

(with-temp-dir dir
               (let [f (path/join dir "a.txt")]
                 (file/write f "data")
                 (assert (nil? (file/sync f)) "syncing a file answers nil")
                 (assert (= (file/read f) "data")
                         "and leaves what the file holds")
                 (assert (nil? (file/sync dir))
                         "syncing a directory answers nil")

                 (let [missing (path/join dir "missing")
                       err (fails-with :io-error (fn [] (file/sync missing)))]
                   (assert (string/contains? (get err :message) missing)
                           "the error names the path"))
                 (fails-with :type-error (fn [] (file/sync 42)))

                 # The trap: an open of a fifo for reading waits for a writer, and nobody
                 # writes this one. A sync that opened its path without O_NONBLOCK would
                 # park until the deadline, and ev/timeout would answer nil.
                 (let [fifo (path/join dir "fifo")]
                   (assert (= 0
                              (subprocess/wait (subprocess/exec "mkfifo" [fifo])))
                           "mkfifo made the fifo")
                   (let [answer (ev/timeout 5 (fn [] (protect (file/sync fifo))))]
                     (assert answer
                             "a fifo answers at once rather than waiting for a writer")
                     (let [[ok? err] answer]
                       (assert (not ok?)
                               "a fifo is neither a file nor a directory")
                       (assert (= (get err :error) :io-error)
                               "so its sync raises :io-error"))))

                 (let [child (fiber/new (fn [] (file/sync f)) |:fs :error|
                                        :deny |:fs|)]
                   (fiber/resume child)
                   (assert (= (fiber/status child) :paused)
                           "a child without :fs traps on file/sync")
                   (assert (= "file/sync" (get (fiber/value child) :primitive))
                           "and the trap names the call"))))

(println "tests/lang/file-sync.lisp: all tests passed")
