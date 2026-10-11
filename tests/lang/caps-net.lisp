(elle/epoch 15)
# audited: 2026-10-11
# Denying :net stops every socket primitive and name lookup, and leaves stdout, timers and a held socket open.
# docs/signals/capabilities.md
#
# Before :net, the one bit that reached the network was :io, and denying it
# also closed stdout, pipes and ev/sleep. Each denial below withholds :net
# alone. The counter-factual is a socket primitive that carries no :net: its
# call runs, and the fiber finishes where it should have parked.

(defn port-of [listener]
  "The port number a TCP listener bound."
  (let [parts (string/split (port/path listener) ":")]
    (parse-int (get parts (- (length parts) 1)))))

(defn denied? [body primitive]
  "Whether body, run in a fiber denied :net, parks on a denial of primitive."
  (let [f (fiber/new body |:net :error| :deny |:net|)]
    (fiber/resume f)
    (let [d (fiber/value f)]
      (and (= (fiber/status f) :paused) (= (get d :error) :capability-denied)
           (= (get d :primitive) primitive) (= (get d :denied) |:net|)))))

(defn declares [f bits]
  "Assert that the primitive f declares exactly bits."
  (let [sig (fn/signature f)]
    (assert (= bits (get (get sig :signals) :bits))
            (string (get sig :name) " declares " bits))))

# ── The bit ───────────────────────────────────────────────────────────

(assert (= (get (signals) :net) 18) "the registry names the network bit")
(assert ((fiber/caps) :net) "the root fiber holds :net")
(let [f (fiber/new (fn [] 42) |:error| :deny |:net|)]
  (assert (not ((fiber/caps f) :net)) "fiber/caps omits :net when denied")
  (assert ((fiber/caps f) :io) ":deny |:net| leaves :io held"))

# ── What each socket primitive declares ───────────────────────────────

(declares tcp/listen |:net :error|)
(declares udp/bind |:net :error|)
(declares tcp/accept |:net :io :error|)
(declares tcp/connect-ip |:net :io :error|)
(declares tcp/shutdown |:net :io :error|)
(declares udp/send-to |:net :io :error|)
(declares udp/recv-from |:net :io :error|)
(declares sys/resolve |:net :io :error|)
(declares unix/accept |:net :io :error|)
(declares unix/shutdown |:net :io :error|)
(declares unix/listen |:fs :net :error|)
(declares unix/connect |:fs :net :io :error|)
(declares sys/ip? ||)
(declares port/read |:io :error|)
(declares port/write |:io :error|)

# ── A denied fiber opens nothing ──────────────────────────────────────

(assert (denied? (fn [] (tcp/listen "127.0.0.1" 0)) "tcp/listen")
        "a denied fiber cannot listen")
(assert (denied? (fn [] (udp/bind "127.0.0.1" 0)) "udp/bind")
        "or bind a datagram socket")
(assert (denied? (fn [] (tcp/connect-ip "127.0.0.1" 9)) "tcp/connect-ip")
        "or connect")
(assert (denied? (fn [] (sys/resolve "localhost")) "sys/resolve")
        "or ask a name server")

(with-temp-dir dir
               (let [path (path/join dir "n.sock")]
                 (assert (denied? (fn [] (unix/listen path)) "unix/listen")
                         "a Unix listen is denied by :net alone")
                 (assert (not (path/exists? path)) "and binds nothing")
                 (assert (denied? (fn [] (unix/connect path)) "unix/connect")
                         "a Unix connect is denied by :net alone")))

# ── A socket handed in is denied what opens or names a peer ───────────

(let [l (tcp/listen "127.0.0.1" 0)]
  (assert (denied? (fn [] (tcp/accept l)) "tcp/accept")
          "a denied fiber cannot accept on a listener it was handed")
  (port/close l))

(let [s (udp/bind "127.0.0.1" 0)]
  (assert (denied? (fn [] (udp/send-to s "x" "127.0.0.1" 9)) "udp/send-to")
          "or send a datagram")
  (assert (denied? (fn [] (udp/recv-from s 16)) "udp/recv-from")
          "or receive one")
  (port/close s))

# ── A connected port is the parent's grant ────────────────────────────

(let* [l (tcp/listen "127.0.0.1" 0)
       server (ev/spawn (fn [] (tcp/accept l)))
       client (tcp/connect-ip "127.0.0.1" (port-of l))
       conn (ev/join server)
       f (fiber/new (fn []
                      (port/write client "ping")
                      (port/read conn 4)) |:error| :deny |:net|)]
  (assert (= (fiber/resume f) (bytes "ping"))
          "a fiber denied :net reads and writes a socket it holds")
  (assert (denied? (fn [] (tcp/shutdown client :write)) "tcp/shutdown")
          "but cannot shut it down")
  (port/close client)
  (port/close conn)
  (port/close l))

# ── Denying :net leaves the rest of :io open ──────────────────────────

(let [f (fiber/new (fn []
                     (ev/sleep 0)
                     (port/flush (port/stdout))
                     :done) |:error| :deny |:net|)]
  (assert (= (fiber/resume f) :done) "timers and stdout stay open")
  (assert (= (fiber/status f) :dead) "and the fiber finishes"))

# Denying :io still closes a call that waits on a peer, which carries both.
(let [f (fiber/new (fn [] (tcp/connect-ip "127.0.0.1" 9)) |:io :error|
                   :deny |:io|)]
  (fiber/resume f)
  (assert (= (get (fiber/value f) :denied) |:io|)
          "a connect is denied :io as well"))

# ── The spend check at io/submit ──────────────────────────────────────

# A fiber that may connect mints the request; a fiber denied :net may not
# spend it. The requirement comes from the connect itself.
(let [l (tcp/listen "127.0.0.1" 0)
      minter (fiber/new (fn [] (tcp/connect-ip "127.0.0.1" (port-of l)))
                        |:io :error|)
      req (fiber/resume minter)
      f (fiber/new (fn [r] (io/submit (io/backend :async) r)) |:net :error|
                   :deny |:net|)]
  (assert (io-request? req) "the minter raised a connect request")
  (fiber/resume f req)
  (let [d (fiber/value f)]
    (assert (= (get d :primitive) "io/submit") "the submit is denied")
    (assert (= (get d :denied) |:net|) "for the :net the connect needs"))
  (port/close l))

(println "caps-net: OK")
