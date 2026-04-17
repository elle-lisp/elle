#!/usr/bin/env elle
## test-mcp.lisp — integration test for tools/mcp-server.lisp
##
## Spawns the MCP server as a subprocess against a freshly-nuked store,
## exercises initialize/tools-list/ping, verifies the startup-population
## of Elle primitives and Rust function triples, then populates, queries,
## and resets user-loaded RDF data through the public tool surface.
##
## The server populates the graph in a background fiber. This test
## verifies that initialize responds within 10 seconds (not blocked
## by population) and that the notifications/model/populated notification
## arrives before graph-dependent tests run.
##
## Usage:
##   elle tools/test-mcp.lisp                   # uses "elle" in PATH
##   elle tools/test-mcp.lisp ./target/debug/elle
##   ELLE_BIN=./target/debug/elle elle tools/test-mcp.lisp

(elle/epoch 5)

## ── Configuration ────────────────────────────────────────────────────────

(def test-args (sys/args))

(def elle-bin
  (cond
    ((not (empty? test-args)) (first test-args))
    ((sys/env "ELLE_BIN")     (sys/env "ELLE_BIN"))
    (true                     "elle")))

(def test-store "./target/elle-mcp-test-store")

## ── Test harness ─────────────────────────────────────────────────────────

(defn test [name ok? msg]
  "Assert a condition; abort the whole suite on first failure."
  (if ok?
    (println "  PASS  " name)
    (begin
      (println "  FAIL  " name " — " msg)
      (error {:error :test-failure :message (string name ": " msg)}))))

(defn rm-rf [path]
  "Recursively delete a path via /bin/rm -rf. No-op if it fails."
  (let [[[ok? _] (protect (subprocess/system "rm" ["-rf" path]))]]
    nil))

## ── JSON-RPC I/O helpers ────────────────────────────────────────────────

(var notification-buffer @[])

(defn send [pin msg]
  "Send a single JSON-RPC message to the server."
  (port/write pin (json/serialize msg))
  (port/write pin "\n")
  (port/flush pin))

(defn recv-response [pout want-id]
  "Read JSON-RPC messages until one with id=want-id arrives.
   Notifications (messages without an id) are saved in the buffer."
  (var result nil)
  (while (nil? result)
    (let [[line (port/read-line pout)]]
      (when (nil? line)
        (error {:error :eof :message "server closed stdout"}))
      (let [[msg (json/parse line)]]
        (if (and (not (nil? (get msg "id"))) (= (get msg "id") want-id))
          (assign result msg)
          (when (not (nil? (get msg "method")))
            (push notification-buffer msg))))))
  result)

(defn recv-notification [pout want-method]
  "Wait for a notification with the given method. Checks buffer first,
   then reads from the port until found."
  (var found nil)
  (var keep @[])
  (each msg in notification-buffer
    (if (and (nil? found) (= (get msg "method") want-method))
      (assign found msg)
      (push keep msg)))
  (assign notification-buffer keep)
  (if (not (nil? found))
    found
    (begin
      (while (nil? found)
        (let [[line (port/read-line pout)]]
          (when (nil? line)
            (error {:error :eof :message "server closed stdout"}))
          (let [[msg (json/parse line)]]
            (if (= (get msg "method") want-method)
              (assign found msg)
              (push notification-buffer msg)))))
      found)))

(defn call-tool [pin pout id name args]
  "Send a tools/call request and wait for the matching response."
  (send pin {:jsonrpc "2.0" :id id :method "tools/call"
             :params {:name name :arguments args}})
  (recv-response pout id))

(defn tool-text [response]
  "Extract the first content[0].text from a tools/call response."
  (get (get (get (get response "result") "content") 0) "text"))

## ── Main ────────────────────────────────────────────────────────────────

(println "── MCP server integration test ──")
(println "  elle-bin:  " elle-bin)
(println "  store:     " test-store)

(rm-rf test-store)

(def proc
  (subprocess/exec elle-bin ["tools/mcp-server.lisp" "--" test-store]))
(def pin  (get proc :stdin))
(def pout (get proc :stdout))

(defer (begin (subprocess/kill proc) (rm-rf test-store))

  ## ── 1. initialize (must respond within 10s — not blocked by population)
  (let [[[ok? r] (protect
      (ev/timeout 10 (fn []
        (send pin {:jsonrpc "2.0" :id 1 :method "initialize"
                   :params {:protocolVersion "2025-03-26"
                            :capabilities {}
                            :clientInfo {:name "test-mcp" :version "0.1"}}})
        (recv-response pout 1))))]]
    (test "initialize: responds within 10 seconds" ok?
      "server took too long — population is blocking startup")
    (when ok?
      (test "initialize: has result"
        (not (nil? (get r "result"))) "missing result")
      (test "initialize: server name is elle-mcp"
        (= (get (get (get r "result") "serverInfo") "name") "elle-mcp")
        (string "got " (get (get (get r "result") "serverInfo") "name")))))

  ## initialized notification — no response expected
  (send pin {:jsonrpc "2.0" :method "notifications/initialized"})

  ## ── 2. tools/list ─────────────────────────────────────────────────────
  (send pin {:jsonrpc "2.0" :id 2 :method "tools/list" :params {}})
  (let [[r (recv-response pout 2)]]
    (let [[tools (get (get r "result") "tools")]]
      (test "tools/list: exposes 21 tools"
        (= (length tools) 21)
        (string "expected 21, got " (length tools)))))

  ## ── 3. ping ───────────────────────────────────────────────────────────
  (send pin {:jsonrpc "2.0" :id 3 :method "ping" :params {}})
  (let [[r (recv-response pout 3)]]
    (test "ping: has result"
      (not (nil? (get r "result"))) "missing result"))

  ## ── 4. drive population to completion ───────────────────────────────────
  ## The server populates the graph one file per request. Send pings to
  ## drive the populator forward until the notification arrives.
  (var ping-id 100)
  (var populated false)
  (while (not populated)
    (send pin {:jsonrpc "2.0" :id ping-id :method "ping" :params {}})
    (var result nil)
    (while (nil? result)
      (let [[line (port/read-line pout)]]
        (when (nil? line)
          (error {:error :eof :message "server closed stdout"}))
        (let [[msg (json/parse line)]]
          (if (= (get msg "method") "notifications/model/populated")
            (begin
              (assign populated true)
              (assign result msg))
            (when (and (not (nil? (get msg "id"))) (= (get msg "id") ping-id))
              (assign result msg))))))
    (assign ping-id (inc ping-id)))

  (test "population: completed" populated "population notification never arrived")

  ## ── 5. startup populated Elle primitives ──────────────────────────────
  (let [[r (call-tool pin pout 5 "sparql_query"
             {:query (string "SELECT (COUNT(?p) AS ?n) WHERE { "
                             "?p <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> "
                             "<urn:elle:Primitive> }")})]]
    (let [[text (tool-text r)]]
      (test "startup: elle primitives are queryable"
        (and (not (nil? text))
             (not (string/contains? text "No results"))
             (not (string/contains? text "SPARQL error")))
        (string "got: " text))))

  ## ── 6. startup populated Rust fn triples ──────────────────────────────
  (let [[r (call-tool pin pout 6 "sparql_query"
             {:query (string "SELECT (COUNT(?f) AS ?n) WHERE { "
                             "?f <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> "
                             "<urn:rust:Fn> }")})]]
    (let [[text (tool-text r)]]
      (test "startup: rust functions are queryable"
        (and (not (nil? text))
             (not (string/contains? text "No results"))
             (not (string/contains? text "SPARQL error")))
        (string "got: " text))))

  ## ── 7. populate: load_rdf with user-owned test triples ────────────────
  (let [[r (call-tool pin pout 7 "load_rdf"
             {:data (string "<http://test/alice> <http://test/name> \"Alice\" .\n"
                            "<http://test/bob> <http://test/name> \"Bob\" .\n")
              :format "ntriples"})]]
    (test "populate: load_rdf succeeded"
      (string/contains? (tool-text r) "successfully")
      (tool-text r)))

  ## ── 8. query: the just-loaded data is visible ────────────────────────
  (let [[r (call-tool pin pout 8 "sparql_query"
             {:query "SELECT ?name WHERE { ?p <http://test/name> ?name } ORDER BY ?name"})]]
    (let [[text (tool-text r)]]
      (test "query: Alice is present"
        (string/contains? text "Alice") text)
      (test "query: Bob is present"
        (string/contains? text "Bob") text)))

  ## ── 9. reset: sparql_update DELETE clears the user triples ────────────
  (let [[r (call-tool pin pout 9 "sparql_update"
             {:update "DELETE WHERE { ?s <http://test/name> ?o }"})]]
    (test "reset: sparql_update delete succeeded"
      (string/contains? (tool-text r) "successfully")
      (tool-text r)))

  ## ── 10. query: user triples are gone after reset ──────────────────────
  (let [[r (call-tool pin pout 10 "sparql_query"
             {:query "SELECT ?name WHERE { ?p <http://test/name> ?name }"})]]
    (let [[text (tool-text r)]]
      (test "reset: user triples are cleared"
        (and (not (string/contains? text "Alice"))
             (not (string/contains? text "Bob")))
        (string "store still has data: " text))))

  ## ── 11. startup-loaded data survives the reset ─────────────────────────
  (let [[r (call-tool pin pout 11 "sparql_query"
             {:query (string "SELECT (COUNT(?p) AS ?n) WHERE { "
                             "?p <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> "
                             "<urn:elle:Primitive> }")})]]
    (let [[text (tool-text r)]]
      (test "reset: startup data is not clobbered"
        (and (not (nil? text))
             (not (string/contains? text "No results")))
        (string "got: " text))))

  ## ── 12. unknown method returns a JSON-RPC error ──────────────────────
  (send pin {:jsonrpc "2.0" :id 12 :method "bogus/method" :params {}})
  (let [[r (recv-response pout 12)]]
    (test "unknown method: returns error object"
      (not (nil? (get r "error"))) "expected error response"))

  (println "")
  (println "all MCP tests passed."))
