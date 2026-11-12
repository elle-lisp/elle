(elle/epoch 15)
# audited: 2026-11-12
## A part that changes while port/write-many waits leaves as the call was given it.
## docs/io/write.md
##
## The peer reads nothing until the writer has parked on a full send buffer.
## Then the program changes the parts: it overwrites the end of an @bytes and
## grows it, which moves its bytes, and it replaces a part of an @array. The
## peer must read the parts as the call was given them. The changes fall at
## the end of the message, which the kernel has not taken when they are made.
##
## The counter-factual: a write that hands the kernel an @bytes where it lies
## sends the overwritten end, and one that reads an @array's parts where they
## lie sends the part put in after the call.
##
## The thread-pool build runs this file too, so both backends answer it.

(def big 2000000)

(defn listen-port [listener]
  "The port number of a listener bound to an ephemeral port."
  (let [parts (string/split (port/path listener) ":")]
    (parse-int (get parts (- (length parts) 1)))))

(defn joined [parts]
  "Every byte of parts, joined in order, as bytes."
  (def @acc (@bytes))
  (each p in parts
    (append acc (bytes p)))
  (freeze acc))

(defn same? [a b]
  "Whether a and b are equal. An assertion over it reports a boolean where one
   over = would print two messages of two megabytes each."
  (= a b))

(defn sent-while-changed [parts change]
  "Write parts over a loopback connection with a 4 KiB send buffer, call
   (change) once the write has parked, and answer [count received]: what the
   call answered, and every byte the peer read."
  (let* [listener (tcp/listen "127.0.0.1" 0)
         acceptor (ev/spawn (fn [] (tcp/accept listener)))
         conn (tcp/connect "127.0.0.1" (listen-port listener) :sndbuf 4096
                           :timeout 5)
         peer (ev/join acceptor)
         writer (ev/spawn (fn []
                            (let [count (port/write-many conn parts)]
                              (port/close conn)
                              count)))]
    (ev/sleep 0.1)
    (change)
    (let [got @[]]
      (forever
        (let [chunk (port/read peer 65536)]
          (when (nil? chunk) (break))
          (push got chunk)))
      (port/close peer)
      (port/close listener)
      [(ev/join writer) (joined got)])))

## ── 1. An @bytes part, overwritten and grown ──────────────────────────

(let* [store (thaw (bytes (string/repeat "a" big)))
       expected (joined [(freeze store) "end"])
       [count received] (sent-while-changed [store "end"]
                          (fn []
                            (each i in (range 4096)
                              (put store (- big 1 i) 98))
                            (append store (bytes (string/repeat "c" big)))))]
  (assert (= count (length expected))
          (string "an @bytes part: the call answered " count " of "
                  (length expected) " bytes"))
  (assert (same? received expected)
          "an @bytes part: the peer read bytes written after the call"))
(println "  1. an @bytes part overwritten and grown leaves as it was given")

## ── 2. A part of an @array, replaced ──────────────────────────────────

(let* [parts @[(bytes (string/repeat "a" big)) (bytes "end")]
       expected (joined (freeze parts))
       [count received] (sent-while-changed parts
                          (fn [] (put parts 1 (bytes "new"))))]
  (assert (= count (length expected))
          (string "an @array: the call answered " count " of " (length expected)
                  " bytes"))
  (assert (same? received expected)
          "an @array: the peer read a part put in after the call"))
(println "  2. a part of an @array replaced leaves as it was given")

(println "port-write-many-changes: all tests passed")
