(elle/epoch 14)
# audited: 2026-09-30
## dns:query and dns:resolve send each query to the :server and :port a caller names.
## lib/dns.lisp
##
## The nameserver in each case is a bound UDP socket that never answers. The
## query gives up, and the test then reads what the socket kept. A query sent
## anywhere else leaves the socket empty, and the read ends at its :timeout.

(def dns ((import "std/dns")))

(defn listen-port [sock]
  "The port number of a socket bound to port 0."
  (let [parts (string/split (port/path sock) ":")]
    (parse-int (get parts (- (length parts) 1)))))

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
  (let [[ok? err] outcome]
    (assert (not ok?) (concat label ": nobody answers, yet it returned"))
    (assert (= (get err :error) :dns-timeout)
            (concat label ": expected :dns-timeout, got " (string err)))))

## ── 1. dns:query sends to :port ──────────────────────────────────────

(let* [server (udp/bind "127.0.0.1" 0)
       outcome (protect (dns:query "port.example.test" dns:TYPE-A
                                   :server "127.0.0.1"
                                   :port (listen-port server) :timeout 300
                                   :retries 1))]
  (assert-query "query" server "port.example.test" dns:TYPE-A)
  (assert-timed-out "query" outcome)
  (port/close server))

(println "  1. dns:query sends to :port")

## ── 2. dns:resolve sends both of its queries to :port ────────────────

(let* [server (udp/bind "127.0.0.1" 0)
       records (dns:resolve "port.example.test" :server "127.0.0.1"
                            :port (listen-port server) :timeout 300 :retries 1)]
  (assert-query "resolve A" server "port.example.test" dns:TYPE-A)
  (assert-query "resolve AAAA" server "port.example.test" dns:TYPE-AAAA)
  (assert (empty? records)
          (concat "resolve: nobody answers, yet it found " (string records)))
  (port/close server))

(println "  2. dns:resolve sends both of its queries to :port")

(println "dns-server: all tests passed")
