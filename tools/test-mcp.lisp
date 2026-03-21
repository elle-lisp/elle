(elle/epoch 1)
#!/usr/bin/env elle
## test-mcp.lisp — smoke test for the MCP server
##
## Spawns the MCP server as a subprocess, sends JSON-RPC messages via
## its stdin, reads responses from its stdout, and asserts correctness.
##
## Run:  elle test-mcp.lisp

(defn test [name ok? msg]
  (if ok?
    (print (string/format "  PASS  {}" name))
    (begin
      (print (string/format "  FAIL  {} — {}" name msg))
      (error {:error :test-failure :message msg}))))

(defn send [pin msg]
  "Send a JSON-RPC message to the server."
  (stream/write pin (json/serialize msg))
  (stream/write pin "\n")
  (stream/flush pin))

(defn recv [pout]
  "Read a JSON-RPC response from the server."
  (let [[line (stream/read-line pout)]]
    (when (nil? line)
      (error {:error :eof :message "server closed stdout"}))
    (json/parse line)))

(ev/run (fn []
  (def proc (subprocess/exec "elle" ["./mcp-server.lisp"]))
  (def pin  (get proc :stdin))
  (def pout (get proc :stdout))
  (def perr (get proc :stderr))

  (defer (subprocess/kill proc)

    # ── 1. Initialize ─────────────────────────────────────────────────
    (send pin {:jsonrpc "2.0" :id 1 :method "initialize"
               :params {:protocolVersion "2025-03-26"
                        :capabilities {}
                        :clientInfo {:name "test" :version "0.1"}}})
    (let [[r (recv pout)]]
      (test "initialize: has result"
        (not (nil? (get r "result"))) "missing result")
      (test "initialize: protocol version"
        (= (get (get r "result") "protocolVersion") "2025-03-26")
        "wrong protocol version")
      (test "initialize: server name"
        (= (get (get (get r "result") "serverInfo") "name") "elle-mcp-oxigraph")
        "wrong server name"))

    # Initialized notification (no response expected)
    (send pin {:jsonrpc "2.0" :method "notifications/initialized"})

    # ── 2. tools/list ──────────────────────────────────────────────────
    (send pin {:jsonrpc "2.0" :id 2 :method "tools/list" :params {}})
    (let [[r (recv pout)]]
      (test "tools/list: has 4 tools"
        (= (length (get (get r "result") "tools")) 4) "expected 4 tools"))

    # ── 3. sparql_update — insert data ─────────────────────────────────
    (send pin {:jsonrpc "2.0" :id 3 :method "tools/call"
               :params {:name "sparql_update"
                        :arguments {:update "INSERT DATA { <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> \"Alice\" . <http://example.org/alice> <http://xmlns.com/foaf/0.1/knows> <http://example.org/bob> . <http://example.org/bob> <http://xmlns.com/foaf/0.1/name> \"Bob\" . }"}}})
    (let [[r (recv pout)]]
      (let [[content (get (get r "result") "content")]]
        (test "sparql_update: success"
          (string/contains? (get (get content 0) "text") "successfully")
          (string (get (get content 0) "text")))))

    # ── 4. sparql_query — read back ────────────────────────────────────
    (send pin {:jsonrpc "2.0" :id 4 :method "tools/call"
               :params {:name "sparql_query"
                        :arguments {:query "SELECT ?name WHERE { ?person <http://xmlns.com/foaf/0.1/name> ?name } ORDER BY ?name"}}})
    (let [[r (recv pout)]]
      (let [[text (get (get (get (get r "result") "content") 0) "text")]]
        (test "sparql_query: has Alice"
          (string/contains? text "Alice") "missing Alice")
        (test "sparql_query: has Bob"
          (string/contains? text "Bob") "missing Bob")))

    # ── 5. load_rdf — bulk load ────────────────────────────────────────
    (send pin {:jsonrpc "2.0" :id 5 :method "tools/call"
               :params {:name "load_rdf"
                        :arguments {:data "<http://example.org/carol> <http://xmlns.com/foaf/0.1/name> \"Carol\" .\n"
                                    :format "ntriples"}}})
    (let [[r (recv pout)]]
      (let [[text (get (get (get (get r "result") "content") 0) "text")]]
        (test "load_rdf: success"
          (string/contains? text "successfully") text)))

    # ── 6. dump_rdf — export ───────────────────────────────────────────
    (send pin {:jsonrpc "2.0" :id 6 :method "tools/call"
               :params {:name "dump_rdf" :arguments {:format "ntriples"}}})
    (let [[r (recv pout)]]
      (let [[text (get (get (get (get r "result") "content") 0) "text")]]
        (test "dump_rdf: has Carol"
          (string/contains? text "Carol") "missing Carol")))

    # ── 7. ping ────────────────────────────────────────────────────────
    (send pin {:jsonrpc "2.0" :id 7 :method "ping" :params {}})
    (let [[r (recv pout)]]
      (test "ping: has result"
        (not (nil? (get r "result"))) "missing result"))

    # ── 8. unknown method ──────────────────────────────────────────────
    (send pin {:jsonrpc "2.0" :id 8 :method "bogus/method" :params {}})
    (let [[r (recv pout)]]
      (test "unknown method: returns error"
        (not (nil? (get r "error"))) "expected error response"))

    (print "")
    (print "all MCP tests passed."))))
