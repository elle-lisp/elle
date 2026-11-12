(elle/epoch 15)
# audited: 2026-11-11
# Two fibers that each write and flush a message on one HTTP or HTTP/2 transport send each message once.
# lib/gather.md
#
# A flush parks its fiber in the write to the port, and the other fiber runs
# meanwhile: it writes its message and flushes too. A transport that empties
# its queue only once its own write returns still holds the first message at
# that moment, so the second flush sends it again, and the peer reads three
# messages for two.
#
# Each message is short, so each write leaves in one syscall and two writes
# cannot interleave inside the descriptor. The order of the two messages is
# the scheduler's, so either order passes.

(def http-transport ((import "std/http/transport")))
(def h2-transport ((import "std/http2/transport")))

(defn listen-port [listener]
  "The port number of a listener bound to an ephemeral port."
  (let [parts (string/split (port/path listener) ":")]
    (parse-int (get parts (- (length parts) 1)))))

(defn heard [wrap]
  "Wrap a loopback connection as a transport with (wrap port), have two fibers
   each write one message on it and flush, and answer every byte the peer
   read, as a string."
  (let* [listener (tcp/listen "127.0.0.1" 0)
         reader (ev/spawn (fn []
                            (let* [conn (tcp/accept listener)
                                   got (port/read-all conn)]
                              (port/close conn)
                              (string got))))
         port (tcp/connect "127.0.0.1" (listen-port listener) :timeout 5)
         t (wrap port)
         send (fn [message]
                (t:write message)
                (t:flush))]
    (ev/join [(ev/spawn (fn [] (send "first\n")))
              (ev/spawn (fn [] (send (bytes "second\n"))))])
    (t:close)
    (let [got (ev/join reader)]
      (port/close listener)
      got)))

(defn each-once? [got]
  (or (= got "first\nsecond\n") (= got "second\nfirst\n")))

(let [got (heard http-transport:tcp)]
  (assert (each-once? got)
          (string "http: the peer read [" got "]" " for two messages")))

(let [got (heard h2-transport:tcp)]
  (assert (each-once? got)
          (string "http2: the peer read [" got "]" " for two messages")))

(println "transport-writes: all tests passed")
