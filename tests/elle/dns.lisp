(elle/epoch 9)
## DNS client tests
##
## Tests the wire-format codec and pure helpers from lib/dns.lisp.

(def dns ((import-file "lib/dns.lisp")))

# ============================================================================
# Internal wire-format tests (pure, no network)
# ============================================================================

(dns:test)

# ============================================================================
# Module export checks
# ============================================================================

(assert (fn? dns:resolve)        "resolve is exported")
(assert (fn? dns:query)          "query is exported")
(assert (fn? dns:parse-response) "parse-response is exported")
(assert (fn? dns:build-query)    "build-query is exported")
(assert (= dns:TYPE-A    1)      "TYPE-A constant")
(assert (= dns:TYPE-AAAA 28)     "TYPE-AAAA constant")
(assert (= dns:TYPE-CNAME 5)     "TYPE-CNAME constant")
(assert (= dns:CLASS-IN  1)      "CLASS-IN constant")

# ============================================================================
# Query building — structural checks
# ============================================================================

# Build a query and parse it back to verify structure
(let [q (dns:build-query 42 "test.example.org" dns:TYPE-A)]
  (assert (bytes? q)              "build-query returns bytes")
  (assert (>= (length q) 12)     "build-query at least header size")

  # Parse our own query's header
  (let [resp (dns:parse-response q)]
    (assert (= resp:header:id 42)      "self-parse: txid")
    (assert (not resp:header:qr)       "self-parse: qr=false (it's a query)")
    (assert (= resp:header:qdcount 1)  "self-parse: 1 question")))

# ============================================================================
# Error cases — header too short
# ============================================================================

(let [[ok? _] (protect (dns:parse-response (bytes 0 0 0)))]
  (assert (not ok?) "parse-response rejects short buffer"))

(println "all dns tests passed")
