#!/usr/bin/env elle

## mcp-server.lisp — MCP server for RDF knowledge graph via SPARQL
##
## A long-running Elle process that speaks the Model Context Protocol
## over stdio (newline-delimited JSON-RPC 2.0).
##
## Tools exposed:
##   sparql_query   — execute SPARQL SELECT / ASK / CONSTRUCT
##   sparql_update  — execute SPARQL UPDATE (INSERT DATA, DELETE, etc.)
##   load_rdf       — load RDF data from a string in turtle/ntriples/nquads/rdfxml
##   dump_rdf       — serialize the store to a string
##
## Usage:
##   elle mcp-server.lisp                        # persistent store in .elle-mcp/store/
##   elle mcp-server.lisp -- /path/to/store      # explicit store path
##   ELLE_MCP_STORE=/path/to/store elle mcp-server.lisp  # via env var

(def ox (import "target/release/libelle_oxigraph.so"))

# ── Store initialization ──────────────────────────────────────────────

(def cli-args (drop 1 (sys/args)))

(def store-path
  (cond
    ((not (empty? cli-args))  (first cli-args))
    ((sys/env "ELLE_MCP_STORE")  (sys/env "ELLE_MCP_STORE"))
    (true  ".elle-mcp/store")))

(file/mkdir-all store-path)

(def store (ox:store-open store-path))

# ── JSON-RPC helpers ──────────────────────────────────────────────────

(defn jsonrpc-result [id result]
  {:jsonrpc "2.0" :id id :result result})

(defn jsonrpc-error [id code message]
  {:jsonrpc "2.0" :id id :error {:code code :message message}})

# ── Logging (to stderr via stream/write — must be called inside ev/run) ──

(defn log [err & parts]
  (stream/write err (string (apply string parts) "\n"))
  (stream/flush err))

# ── Tool definitions (MCP schema) ─────────────────────────────────────

(def tool-sparql-query
  {:name "sparql_query"
   :description "Execute a SPARQL query (SELECT, ASK, or CONSTRUCT) against the RDF knowledge graph. SELECT returns an array of binding rows. ASK returns a boolean. CONSTRUCT returns an array of quads."
   :inputSchema {:type "object"
                 :properties {:query {:type "string"
                                      :description "SPARQL query string"}}
                 :required ["query"]}})

(def tool-sparql-update
  {:name "sparql_update"
   :description "Execute a SPARQL UPDATE operation (INSERT DATA, DELETE DATA, DELETE/INSERT WHERE, LOAD, CLEAR, DROP, etc.) against the RDF knowledge graph."
   :inputSchema {:type "object"
                 :properties {:update {:type "string"
                                       :description "SPARQL Update string"}}
                 :required ["update"]}})

(def tool-load-rdf
  {:name "load_rdf"
   :description "Load RDF data from a string into the knowledge graph. Supported formats: turtle, ntriples, nquads, rdfxml."
   :inputSchema {:type "object"
                 :properties {:data {:type "string"
                                     :description "RDF data as a string"}
                              :format {:type "string"
                                       :enum ["turtle" "ntriples" "nquads" "rdfxml"]
                                       :description "RDF serialization format"}}
                 :required ["data" "format"]}})

(def tool-dump-rdf
  {:name "dump_rdf"
   :description "Serialize the RDF knowledge graph to a string. Use nquads for all graphs or turtle/ntriples/rdfxml for the default graph only."
   :inputSchema {:type "object"
                 :properties {:format {:type "string"
                                       :enum ["turtle" "ntriples" "nquads" "rdfxml"]
                                       :description "RDF serialization format (default: turtle)"}}
                 :required []}})

(def all-tools [tool-sparql-query tool-sparql-update tool-load-rdf tool-dump-rdf])

# ── Tool execution ────────────────────────────────────────────────────

(defn format-query-results [results]
  "Format oxigraph query results into a readable string for MCP."
  (cond
    ((boolean? results)
     (if results "true" "false"))

    ((array? results)
     (if (empty? results)
       "No results."
       (json/pretty results)))

    (true (string results))))

(defn call-sparql-query [arguments]
  (let [[query (get arguments "query")]]
    (when (nil? query)
      (error {:error :invalid-params :message "missing required parameter: query"}))
    (let [[[ok? result] (protect (ox:query store query))]]
      (if ok?
        [{:type "text" :text (format-query-results result)}]
        [{:type "text" :text (string/format "SPARQL error: {}" (get result :message)) :isError true}]))))

(defn call-sparql-update [arguments]
  (let [[update-str (get arguments "update")]]
    (when (nil? update-str)
      (error {:error :invalid-params :message "missing required parameter: update"}))
    (let [[[ok? result] (protect (ox:update store update-str))]]
      (if ok?
        [{:type "text" :text "Update executed successfully."}]
        [{:type "text" :text (string/format "SPARQL update error: {}" (get result :message)) :isError true}]))))

(defn call-load-rdf [arguments]
  (let [[data (get arguments "data")]
        [fmt  (get arguments "format")]]
    (when (nil? data)
      (error {:error :invalid-params :message "missing required parameter: data"}))
    (when (nil? fmt)
      (error {:error :invalid-params :message "missing required parameter: format"}))
    (let* [[fmt-kw (keyword fmt)]
           [[ok? result] (protect (ox:load store data fmt-kw))]]
      (if ok?
        [{:type "text" :text "RDF data loaded successfully."}]
        [{:type "text" :text (string/format "Load error: {}" (get result :message)) :isError true}]))))

(defn call-dump-rdf [arguments]
  (let* [[fmt (or (get arguments "format") "turtle")]
         [fmt-kw (keyword fmt)]
         [[ok? result] (protect (ox:dump store fmt-kw))]]
    (if ok?
      [{:type "text" :text result}]
      [{:type "text" :text (string/format "Dump error: {}" (get result :message)) :isError true}])))

(defn dispatch-tool [name arguments]
  (case name
    "sparql_query"  (call-sparql-query arguments)
    "sparql_update" (call-sparql-update arguments)
    "load_rdf"      (call-load-rdf arguments)
    "dump_rdf"      (call-dump-rdf arguments)
    (error {:error :method-not-found
            :message (string/format "unknown tool: {}" name)})))

# ── MCP method dispatch ───────────────────────────────────────────────

(defn handle-initialize [id _params]
  (jsonrpc-result id
    {:protocolVersion "2025-03-26"
     :capabilities {:tools {:listChanged false}}
     :serverInfo {:name "elle-mcp-oxigraph"
                  :version "0.1.0"}
     :instructions "RDF knowledge graph server. Use sparql_query to read and sparql_update to write triples. Use load_rdf/dump_rdf for bulk import/export."}))

(defn handle-tools-list [id _params]
  (jsonrpc-result id {:tools all-tools}))

(defn handle-tools-call [id params]
  (let [[name (get params "name")]
        [arguments (or (get params "arguments") {})]]
    (let [[[ok? content] (protect (dispatch-tool name arguments))]]
      (if ok?
        (jsonrpc-result id {:content content})
        (jsonrpc-result id {:content [{:type "text"
                                       :text (string/format "Internal error: {}" (get content :message))
                                       :isError true}]
                            :isError true})))))

(defn handle-ping [id _params]
  (jsonrpc-result id {}))

(defn handle-request [msg]
  "Dispatch a JSON-RPC request and return a response, or nil for notifications."
  (let [[method (get msg "method")]
        [id     (get msg "id")]
        [params (or (get msg "params") {})]]
    (if (nil? id)
      nil
      (case method
        "initialize"    (handle-initialize id params)
        "ping"          (handle-ping id params)
        "tools/list"    (handle-tools-list id params)
        "tools/call"    (handle-tools-call id params)
        (jsonrpc-error id -32601 (string/format "method not found: {}" method))))))

# ── Main loop ─────────────────────────────────────────────────────────

(defn send-response [out response]
  "Write a JSON-RPC response as a single line to stdout and flush."
  (stream/write out (string (json/serialize response) "\n"))
  (stream/flush out))

(ev/run
  (fn []
    (let [[in  (*stdin*)]
          [out (*stdout*)]
          [err (*stderr*)]]
      (log err "elle-mcp-oxigraph server starting")
      (forever
        (let [[line (stream/read-line in)]]
          (when (nil? line)
            (log err "stdin closed, shutting down")
            (break))
          (unless (empty? line)
            (let [[[ok? msg] (protect (json/parse line))]]
              (if (not ok?)
                (begin
                  (log err "JSON parse error: " (get msg :message))
                  (send-response out (jsonrpc-error nil -32700 "parse error")))
                (let [[response (handle-request msg)]]
                  (unless (nil? response)
                    (send-response out response)))))))))))
