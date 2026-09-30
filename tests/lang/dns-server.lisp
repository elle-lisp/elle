(elle/epoch 14)
# audited: 2026-09-30
## dns:query and dns:resolve send each query to the :server and :port a caller names, and wait :timeout seconds for an answer.
## lib/dns.lisp
##
## The nameserver in each case is a bound UDP socket that never answers. The
## query gives up, and the test then reads what the socket kept. A query sent
## anywhere else leaves the socket empty, and the read ends at its :timeout.
##
## The counter-factual for the bound is a :timeout read in milliseconds:
## :timeout 0.3 then gives up after 0.3 ms, well before the 0.3 s asserted.

(def dns ((import "std/dns")))

(defn listen-port [sock]
  "The port number of a socket bound to port 0."
  (let [parts (string/split (port/path sock) ":")]
    (parse-int (get parts (- (length parts) 1)))))

(defn timed [thunk]
  "Run `thunk` under protect. Returns [ok? result elapsed-seconds]."
  (let* [started (clock/monotonic)
         [ok? result] (protect (thunk))]
    [ok? result (- (clock/monotonic) started)]))

(defn assert-query [label server name qtype]
  "Assert that `server` holds a query for `name` and `qtype`, and consume it."
  (let [[ok? got] (protect (udp/recv-from server 512 :timeout 1))]
    (assert ok?
            (concat label ": the server on :port received no query, got "
                    (string got)))
    (let [txid (get (get (dns:parse-response got:data) :header) :id)]
      (assert (= got:data (dns:build-query txid name qtype))
              (concat label ": the server received another datagram")))))

(defn assert-timed-out [label outcome]
  "Assert that a query nobody answers raised :dns-timeout."
  (let [[ok? err _] outcome]
    (assert (not ok?) (concat label ": nobody answers, yet it returned"))
    (assert (= (get err :error) :dns-timeout)
            (concat label ": expected :dns-timeout, got " (string err)))))

(defn assert-waited [label outcome at-least below]
  "Assert that the call took `at-least` seconds, and less than `below`."
  (let [elapsed (get outcome 2)]
    (assert (>= elapsed at-least)
            (concat label ": gave up after " (string elapsed)
                    " s, before its bound of " (string at-least) " s"))
    (assert (< elapsed below)
            (concat label ": took " (string elapsed) " s, past its bound of "
                    (string at-least) " s"))))

## ── 1. dns:query sends to :port and waits :timeout ───────────────────

(let* [server (udp/bind "127.0.0.1" 0)
       outcome (timed (fn []
                        (dns:query "port.example.test" dns:TYPE-A
                                   :server "127.0.0.1"
                                   :port (listen-port server) :timeout 0.3
                                   :retries 1)))]
  (assert-query "query" server "port.example.test" dns:TYPE-A)
  (assert-timed-out "query" outcome)
  (assert-waited "query" outcome 0.3 3)
  (port/close server))

(println "  1. dns:query sends to :port and waits :timeout seconds")

## ── 2. dns:resolve sends both of its queries to :port ────────────────

(let* [server (udp/bind "127.0.0.1" 0)
       outcome (timed (fn []
                        (dns:resolve "port.example.test" :server "127.0.0.1"
                                     :port (listen-port server) :timeout 0.3
                                     :retries 1)))
       [ok? records _] outcome]
  (assert-query "resolve A" server "port.example.test" dns:TYPE-A)
  (assert-query "resolve AAAA" server "port.example.test" dns:TYPE-AAAA)
  (assert ok?
          (concat "resolve: raised instead of returning no records: "
                  (string records)))
  (assert (empty? records)
          (concat "resolve: nobody answers, yet it found " (string records)))
  (assert-waited "resolve" outcome 0.6 3)
  (port/close server))

(println "  2. dns:resolve sends both of its queries to :port")

## ── 3. :retries sends the query that many times ──────────────────────

(let* [server (udp/bind "127.0.0.1" 0)
       outcome (timed (fn []
                        (dns:query "retry.example.test" dns:TYPE-A
                                   :server "127.0.0.1"
                                   :port (listen-port server) :timeout 0.3
                                   :retries 2)))]
  (assert-query "first send" server "retry.example.test" dns:TYPE-A)
  (assert-query "second send" server "retry.example.test" dns:TYPE-A)
  (assert-timed-out "retries" outcome)
  (assert-waited "retries" outcome 0.6 3)
  (port/close server))

(println "  3. :retries 2 sends the query twice and waits for each")

## ── 4. the default :timeout is 3 seconds ─────────────────────────────
##
## ev/timeout returns nil when its 10 seconds run out first, so a default read
## as 3000 seconds fails assert-timed-out instead of hanging the file.

(let* [server (udp/bind "127.0.0.1" 0)
       outcome (timed (fn []
                        (ev/timeout 10
                                    (fn []
                                      (dns:query "default.example.test"
                                      dns:TYPE-A :server "127.0.0.1"
                                      :port (listen-port server) :retries 1)))))]
  (assert-timed-out "default" outcome)
  (assert-waited "default" outcome 3 10)
  (port/close server))

(println "  4. the default :timeout is 3 seconds")

(println "dns-server: all tests passed")
