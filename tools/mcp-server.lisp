#!/usr/bin/env elle

## mcp-server.lisp — MCP server: RDF knowledge graph + semantic analysis
##
## Tools exposed:
##   ping             — verify the server is alive
##   sparql_query     — execute SPARQL SELECT / ASK / CONSTRUCT
##   sparql_update    — execute SPARQL UPDATE
##   load_rdf         — load RDF data from a string
##   dump_rdf         — serialize the store to a string
##   analyze_file     — analyze an Elle file and populate the graph
##   portrait         — semantic portrait of a function or module
##   signal_query     — find functions by signal property
##   impact           — assess the impact of changing a function
##   verify_invariants — check project invariants
##   compile_rename   — binding-aware rename
##   compile_extract  — extract a line range into a new function
##   compile_parallelize — check if functions can safely run in parallel
##   trace            — trace an Elle function through primitives into Rust
##
## Usage:
##   elle mcp-server.lisp                        # store in .elle-mcp/store/
##   elle mcp-server.lisp -- /path/to/store      # explicit store path
##   ELLE_MCP_STORE=/path/to/store elle mcp-server.lisp

# ── Stdio + imports ──────────────────────────────────────────────────────
# Capture ports before plugin imports (RocksDB may redirect fds).

(def saved-stdin  (*stdin*))
(def saved-stdout (*stdout*))
(def saved-stderr (*stderr*))

(def ox (import "oxigraph"))
(def syn (import "syn"))
(def glob-plugin (import "glob"))
(def portrait-lib ((import "lib/portrait")))
(def rdf ((import "lib/rdf")))
(def rust-rdf ((import "tools/rust-rdf-lib") syn))

# ── Watch library ───────────────────────────────────────────────────────

(def watch ((import "lib/watch")))

# ── Store initialization ─────────────────────────────────────────────────

(def cli-args (drop 1 (sys/args)))

(def store-path
  (cond
    ((not (empty? cli-args))       (first cli-args))
    ((sys/env "ELLE_MCP_STORE")    (sys/env "ELLE_MCP_STORE"))
    (true                          ".elle-mcp/store")))

(defn nuke-store [path]
  "Delete a corrupt store directory so it can be recreated fresh."
  (each entry in (glob-plugin:glob (string path "/*"))
    (file/delete entry))
  nil)

(defn open-store [path]
  "Open the oxigraph store, nuking and recreating if corrupt."
  (file/mkdir-all path)
  (let [[[ok? s] (protect (ox:store-open path))]]
    (if ok? s
      (begin
        (eprintln "store corrupt, rebuilding: " path)
        (nuke-store path)
        (ox:store-open path)))))

(def store (open-store store-path))

(defn flush-store []
  (ox:store-flush store))

# ── Graph population ─────────────────────────────────────────────────────

(defn populate-primitives []
  "Load Elle primitive triples into the RDF store."
  (ox:load store (rdf:primitives) :ntriples)
  (flush-store))

(defn populate-rust []
  "Parse all .rs files, load Rust triples and primitive cross-links."
  (var files (glob-plugin:glob "**/*.rs"))
  (var count 0)
  (each file in files
    (let [[[ok? _err] (protect
            (begin
              (ox:load store (rust-rdf:file file) :ntriples)
              (ox:load store (rust-rdf:primitive-links file) :ntriples)))]]
      (when ok? (assign count (inc count)))))
  (flush-store)
  count)

(defn clear-file-triples [path]
  "Remove all triples for a file from the RDF store."
  (ox:update store
    (string/format "DELETE WHERE {{ ?s <urn:elle:file> \"{}\" . ?s ?p ?o . }}"
      (string/replace path "\\" "\\\\"))))

(defn populate-file [analysis path]
  "Load triples for an analyzed file, replacing any existing."
  (clear-file-triples path)
  (ox:load store (rdf:file analysis path) :ntriples)
  (flush-store))

(populate-primitives)
(def rust-file-count (populate-rust))

# ── Analysis cache ───────────────────────────────────────────────────────

(def analysis-cache @{})

(defn get-or-analyze [path]
  "Return cached analysis or analyze the file fresh."
  (var cached (get analysis-cache path))
  (if (not (nil? cached))
    cached
    (begin
      (var result (compile/analyze (file/read path) {:file path}))
      (put analysis-cache path result)
      (populate-file result path)
      result)))

(defn invalidate-cache [path]
  "Remove a path from the analysis cache so it is re-analyzed on next access."
  (put analysis-cache path nil))

# ── Signal diff (for watch notifications) ────────────────────────────────

(defn snapshot-signals [analysis]
  "Build a map of function-name -> signal struct for diff comparison."
  (var result @{})
  (each sym in (compile/symbols analysis)
    (when (= (get sym :kind) :function)
      (var name (get sym :name))
      (let [[[ok? sig] (protect (compile/signal analysis (keyword name)))]]
        (when ok? (put result name sig)))))
  result)

(defn diff-signals [old-sigs new-sigs]
  "Compare two signal snapshots, return {:added :removed :changed}."
  (var added @[])
  (var removed @[])
  (var changed @[])
  (each name in (keys new-sigs)
    (var old-sig (get old-sigs name))
    (var new-sig (get new-sigs name))
    (if (nil? old-sig)
      (push added name)
      (unless (= old-sig new-sig)
        (push changed {:name name :before old-sig :after new-sig}))))
  (each name in (keys old-sigs)
    (when (nil? (get new-sigs name))
      (push removed name)))
  {:added (freeze added) :removed (freeze removed) :changed (freeze changed)})

(defn build-notification [path diff]
  "Build a model/updated JSON-RPC notification."
  (var changes @[])
  (each c in (get diff :changed)
    (var before (get c :before))
    (var after  (get c :after))
    (each field in [:silent :io :yields :errors :exec]
      (when (not (= (get before field) (get after field)))
        (push changes {:name (get c :name) :field (string field)
                       :before (get before field) :after (get after field)}))))
  {:jsonrpc "2.0"
   :method "notifications/model/updated"
   :params {:file path
            :functions_added (get diff :added)
            :functions_removed (get diff :removed)
            :signal_changes (freeze changes)}})

# ── Rebind stdio after imports ───────────────────────────────────────────

(parameterize ((*stdin* saved-stdin) (*stdout* saved-stdout) (*stderr* saved-stderr))

# ── JSON-RPC helpers ─────────────────────────────────────────────────────

(defn jsonrpc-result [id result]
  {:jsonrpc "2.0" :id id :result result})

(defn jsonrpc-error [id code message]
  {:jsonrpc "2.0" :id id :error {:code code :message message}})

(defn text-content [text]
  [{:type "text" :text text}])

(defn error-content [text]
  [{:type "text" :text text :isError true}])

(defn send-response [response]
  (println (json/serialize response)))

(defn ->list [coll]
  "Convert any collection to a list for string/join."
  (var result ())
  (each x in coll (assign result (cons x result)))
  result)

# ── Tool definitions ─────────────────────────────────────────────────────

(def tool-ping
  {:name "ping"
   :description "Return pong. Use to verify the server is alive."
   :inputSchema {:type "object" :properties {} :required []}})

(def tool-sparql-query
  {:name "sparql_query"
   :description "Execute a SPARQL query (SELECT, ASK, or CONSTRUCT) against the RDF knowledge graph."
   :inputSchema {:type "object"
                 :properties {:query {:type "string"
                                      :description "SPARQL query string"}}
                 :required ["query"]}})

(def tool-sparql-update
  {:name "sparql_update"
   :description "Execute a SPARQL UPDATE operation against the RDF knowledge graph."
   :inputSchema {:type "object"
                 :properties {:update {:type "string"
                                       :description "SPARQL Update string"}}
                 :required ["update"]}})

(def tool-load-rdf
  {:name "load_rdf"
   :description "Load RDF data from a string into the knowledge graph. Formats: turtle, ntriples, nquads, rdfxml."
   :inputSchema {:type "object"
                 :properties {:data   {:type "string" :description "RDF data as a string"}
                              :format {:type "string"
                                       :enum ["turtle" "ntriples" "nquads" "rdfxml"]
                                       :description "RDF serialization format"}}
                 :required ["data" "format"]}})

(def tool-dump-rdf
  {:name "dump_rdf"
   :description "Serialize the RDF knowledge graph to a string."
   :inputSchema {:type "object"
                 :properties {:format {:type "string"
                                       :enum ["turtle" "ntriples" "nquads" "rdfxml"]
                                       :description "RDF serialization format (default: turtle)"}}
                 :required []}})

(def tool-analyze-file
  {:name "analyze_file"
   :description "Analyze an Elle source file. Returns a summary of symbols, signals, diagnostics, and observations. Populates the RDF graph."
   :inputSchema {:type "object"
                 :properties {:path {:type "string" :description "Path to the .lisp file"}}
                 :required ["path"]}})

(def tool-portrait
  {:name "portrait"
   :description "Semantic portrait of a function or module. Shows effect profile, failure modes, composition properties, and observations."
   :inputSchema {:type "object"
                 :properties {:path     {:type "string" :description "Path to the .lisp file"}
                              :function {:type "string" :description "Function name (omit for module portrait)"}}
                 :required ["path"]}})

(def tool-signal-query
  {:name "signal_query"
   :description "Find functions matching a signal property: silent, io, yields, jit-eligible, errors, or any signal keyword."
   :inputSchema {:type "object"
                 :properties {:path  {:type "string" :description "Path to the .lisp file"}
                              :query {:type "string" :description "Signal property to match"}}
                 :required ["path" "query"]}})

(def tool-impact
  {:name "impact"
   :description "Assess the impact of changing a function. Shows callers, downstream signal implications, and JIT eligibility."
   :inputSchema {:type "object"
                 :properties {:path     {:type "string" :description "Path to the .lisp file"}
                              :function {:type "string" :description "Function name to assess"}}
                 :required ["path" "function"]}})

(def tool-verify-invariants
  {:name "verify_invariants"
   :description "Check project invariants encoded as SPARQL ASK queries."
   :inputSchema {:type "object"
                 :properties {:path {:type "string"
                                     :description "Path to invariants file (default: .elle-invariants.lisp)"}}
                 :required []}})

(def tool-compile-rename
  {:name "compile_rename"
   :description "Binding-aware rename of a function or variable and all its references."
   :inputSchema {:type "object"
                 :properties {:path     {:type "string" :description "Path to Elle source file"}
                              :old_name {:type "string" :description "Current name"}
                              :new_name {:type "string" :description "New name"}}
                 :required ["path" "old_name" "new_name"]}})

(def tool-compile-extract
  {:name "compile_extract"
   :description "Extract a line range into a new function. Computes free variables and signal."
   :inputSchema {:type "object"
                 :properties {:path       {:type "string"  :description "Path to Elle source file"}
                              :from       {:type "string"  :description "Function to extract from"}
                              :start_line {:type "integer" :description "Start line (1-indexed)"}
                              :end_line   {:type "integer" :description "End line (1-indexed)"}
                              :name       {:type "string"  :description "Name for extracted function"}}
                 :required ["path" "from" "start_line" "end_line" "name"]}})

(def tool-compile-parallelize
  {:name "compile_parallelize"
   :description "Check if functions can safely run in parallel. Verifies no shared mutable captures."
   :inputSchema {:type "object"
                 :properties {:path      {:type "string" :description "Path to Elle source file"}
                              :functions {:type "array" :items {:type "string"}
                                          :description "Function names to check"}}
                 :required ["path" "functions"]}})

(def tool-trace
  {:name "trace"
   :description "Trace an Elle function through primitives into Rust implementation. Shows the full call chain: Elle code -> Elle primitives -> Rust functions -> deeper Rust calls."
   :inputSchema {:type "object"
                 :properties {:path     {:type "string" :description "Path to the .lisp file"}
                              :function {:type "string" :description "Function name to trace"}
                              :depth    {:type "integer" :description "Max Rust call depth (default: 2)"}}
                 :required ["path" "function"]}})

(def all-tools
  [tool-ping tool-sparql-query tool-sparql-update tool-load-rdf tool-dump-rdf
   tool-analyze-file tool-portrait tool-signal-query tool-impact
   tool-verify-invariants tool-compile-rename tool-compile-extract
   tool-compile-parallelize tool-trace])

# ── SPARQL tool handlers ─────────────────────────────────────────────────

(defn call-sparql-query [arguments]
  (let [[query (get arguments "query")]]
    (when (nil? query)
      (error {:error :invalid-params :message "missing required parameter: query"}))
    (let [[[ok? result] (protect (ox:query store query))]]
      (if ok?
        (text-content (cond
          ((boolean? result) (if result "true" "false"))
          ((array? result)   (if (empty? result) "No results." (json/pretty result)))
          (true              (string result))))
        (error-content (string/format "SPARQL error: {}" (get result :message)))))))

(defn call-sparql-update [arguments]
  (let [[update-str (get arguments "update")]]
    (when (nil? update-str)
      (error {:error :invalid-params :message "missing required parameter: update"}))
    (let [[[ok? result] (protect (ox:update store update-str))]]
      (if ok?
        (begin (flush-store) (text-content "Update executed successfully."))
        (error-content (string/format "SPARQL update error: {}" (get result :message)))))))

(defn call-load-rdf [arguments]
  (let [[data (get arguments "data")]
        [fmt  (get arguments "format")]]
    (when (nil? data)
      (error {:error :invalid-params :message "missing required parameter: data"}))
    (when (nil? fmt)
      (error {:error :invalid-params :message "missing required parameter: format"}))
    (let [[[ok? result] (protect (ox:load store data (keyword fmt)))]]
      (if ok?
        (begin (flush-store) (text-content "RDF data loaded successfully."))
        (error-content (string/format "Load error: {}" (get result :message)))))))

(defn call-dump-rdf [arguments]
  (let* [[fmt (or (get arguments "format") "turtle")]
         [[ok? result] (protect (ox:dump store (keyword fmt)))]]
    (if ok?
      (text-content result)
      (error-content (string/format "Dump error: {}" (get result :message))))))

# ── Semantic tool handlers ───────────────────────────────────────────────

(defn call-analyze-file [arguments]
  (var path (get arguments "path"))
  (when (nil? path)
    (error {:error :invalid-params :message "missing required parameter: path"}))

  (var analysis (get-or-analyze path))
  (var syms (compile/symbols analysis))
  (var diags (compile/diagnostics analysis))
  (var fn-syms (filter (fn [s] (= (get s :kind) :function)) syms))

  (var silent-names @[])
  (var io-names @[])
  (var delegating-names @[])
  (var yielding-names @[])
  (var observations @[])

  (each sym in fn-syms
    (var name (get sym :name))
    (var sig nil)
    (let [[[ok? val] (protect (compile/signal analysis (keyword name)))]]
      (when ok? (assign sig val)))
    (when sig
      (cond
        ((get sig :silent)                         (push silent-names name))
        ((not (empty? (get sig :propagates)))      (push delegating-names name))
        ((get sig :io)                             (push io-names name))
        ((get sig :yields)                         (push yielding-names name))
        (true                                      (push io-names name)))

      (var caps nil)
      (let [[[ok? val] (protect (compile/captures analysis (keyword name)))]]
        (when ok? (assign caps val)))
      (when caps
        (var callees nil)
        (let [[[ok? val] (protect (compile/callees analysis (keyword name)))]]
          (when ok? (assign callees val)))
        (when callees
          (each o in (portrait-lib:observations analysis name sig caps callees)
            (push observations
              (string/format "{}: [{}] {}" name (get o :kind) (get o :message))))))))

  (var out @"")
  (push out (string/format "Analyzed {} (graph populated)\n\n" path))
  (push out (string/format "Functions: {}\n" (length fn-syms)))

  (when (not (empty? silent-names))
    (push out (string/format "\n  Silent ({}): {}\n"
      (length silent-names) (string/join (->list silent-names) ", "))))
  (when (not (empty? io-names))
    (push out (string/format "  I/O ({}): {}\n"
      (length io-names) (string/join (->list io-names) ", "))))
  (when (not (empty? delegating-names))
    (push out (string/format "  Delegating ({}): {}\n"
      (length delegating-names) (string/join (->list delegating-names) ", "))))
  (when (not (empty? yielding-names))
    (push out (string/format "  Yielding ({}): {}\n"
      (length yielding-names) (string/join (->list yielding-names) ", "))))

  (when (not (empty? diags))
    (push out "\nDiagnostics:\n")
    (each d in diags
      (push out (string/format "  [{}] {} (line {})\n"
        (get d :severity) (get d :message) (or (get d :line) "?")))))

  (when (not (empty? observations))
    (push out "\nObservations:\n")
    (each o in observations
      (push out (string/format "  - {}\n" o))))

  (text-content (freeze out)))

(defn call-portrait [arguments]
  (var path (get arguments "path"))
  (when (nil? path)
    (error {:error :invalid-params :message "missing required parameter: path"}))
  (var analysis (get-or-analyze path))
  (var fn-name (get arguments "function"))
  (if (nil? fn-name)
    (text-content (portrait-lib:render-module (portrait-lib:module analysis)))
    (text-content (portrait-lib:render (portrait-lib:function analysis (keyword fn-name))))))

(defn call-signal-query [arguments]
  (var path (get arguments "path"))
  (var query (get arguments "query"))
  (when (nil? path)
    (error {:error :invalid-params :message "missing required parameter: path"}))
  (when (nil? query)
    (error {:error :invalid-params :message "missing required parameter: query"}))
  (var analysis (get-or-analyze path))
  (var matches (compile/query-signal analysis (keyword query)))
  (var out @"")
  (push out (string/format "Functions matching '{}' in {}:\n\n" query path))
  (if (empty? matches)
    (push out "  (none)\n")
    (each m in matches
      (push out (string/format "  {} (line {})\n"
        (get m :name) (or (get m :line) "?")))))
  (text-content (freeze out)))

(defn call-impact [arguments]
  (var path (get arguments "path"))
  (var fn-name (get arguments "function"))
  (when (nil? path)
    (error {:error :invalid-params :message "missing required parameter: path"}))
  (when (nil? fn-name)
    (error {:error :invalid-params :message "missing required parameter: function"}))
  (var analysis (get-or-analyze path))
  (var sig (compile/signal analysis (keyword fn-name)))
  (var callers (compile/callers analysis (keyword fn-name)))
  (var callees (compile/callees analysis (keyword fn-name)))

  (var out @"")
  (push out (string/format "Impact analysis for '{}' in {}\n\n" fn-name path))
  (push out (string/format "Current signal: {}\n" sig))
  (push out (string/format "  silent={} yields={} io={}\n\n"
    (get sig :silent) (get sig :yields) (get sig :io)))

  (push out (string/format "Called by ({} callers):\n" (length callers)))
  (each c in callers
    (var caller-name (get c :name))
    (var caller-sig nil)
    (let [[[ok? val] (protect (compile/signal analysis (keyword caller-name)))]]
      (when ok? (assign caller-sig val)))
    (push out (string/format "  {} (line {}, tail={})"
      caller-name (or (get c :line) "?") (or (get c :tail) false)))
    (when caller-sig
      (when (get caller-sig :silent)
        (push out " ! caller is silent — adding effects here will propagate"))
      (when (get caller-sig :jit-eligible)
        (push out " ! caller is JIT-eligible — adding yields will disable JIT")))
    (push out "\n"))

  (push out (string/format "\nCalls ({} callees):\n" (length callees)))
  (each c in callees
    (push out (string/format "  {} (line {}, tail={})\n"
      (get c :name) (or (get c :line) "?") (or (get c :tail) false))))

  (var caps (compile/captures analysis (keyword fn-name)))
  (when (not (empty? caps))
    (push out "\nCaptures:\n")
    (each cap in caps
      (push out (string/format "  {} ({}{})\n"
        (get cap :name) (get cap :kind)
        (if (get cap :mutated) ", mutable" "")))))

  (text-content (freeze out)))

(defn call-verify-invariants [arguments]
  (var inv-path (or (get arguments "path") ".elle-invariants.lisp"))
  (var src nil)
  (let [[[ok? result] (protect (file/read inv-path))]]
    (if ok?
      (assign src result)
      (error {:error :io-error
              :message (string/format "cannot read invariants file: {}" inv-path)})))
  (var invariants nil)
  (let [[[ok? result] (protect (eval (read src)))]]
    (if ok?
      (assign invariants result)
      (error {:error :parse-error
              :message (string/format "cannot parse invariants: {}" (get result :message))})))

  (var out @"")
  (var pass-count 0)
  (var fail-count 0)
  (each inv in invariants
    (var name (get inv :name))
    (var query (get inv :query))
    (var expected (get inv :expect))
    (var actual nil)
    (let [[[ok? result] (protect (ox:query store query))]]
      (if ok?
        (assign actual result)
        (begin
          (push out (string/format "  x {} — query error: {}\n" name (get result :message)))
          (assign fail-count (+ fail-count 1)))))
    (when (not (nil? actual))
      (if (= actual expected)
        (begin
          (push out (string/format "  ok {}\n" name))
          (assign pass-count (+ pass-count 1)))
        (begin
          (push out (string/format "  x {} — expected {}, got {}\n" name expected actual))
          (assign fail-count (+ fail-count 1))))))
  (text-content (string (string/format "Invariant check: {} passed, {} failed\n\n"
    pass-count fail-count) (freeze out))))

# ── Transformation tool handlers ─────────────────────────────────────────

(defn call-compile-rename [arguments]
  (var path (get arguments "path"))
  (var old-name (get arguments "old_name"))
  (var new-name (get arguments "new_name"))
  (var analysis (get-or-analyze path))
  (text-content (json/serialize
    (compile/rename analysis (keyword old-name) (keyword new-name)))))

(defn call-compile-extract [arguments]
  (var path (get arguments "path"))
  (var analysis (get-or-analyze path))
  (text-content (json/serialize
    (compile/extract analysis
      {:from  (keyword (get arguments "from"))
       :lines [(get arguments "start_line") (get arguments "end_line")]
       :name  (keyword (get arguments "name"))}))))

(defn call-compile-parallelize [arguments]
  (var path (get arguments "path"))
  (var analysis (get-or-analyze path))
  (text-content (json/serialize
    (compile/parallelize analysis (map keyword (get arguments "functions"))))))

# ── Trace tool handler ────────────────────────────────────────────────────

(defn rdf-val [term]
  "Extract the value from an RDF term like [\"literal\" \"x\"] or [\"iri\" \"x\"]."
  (get term 1))

(defn query-rust-callees [rust-fn depth max-depth]
  "Recursively query Rust call edges, return a tree of {:name :file :line :iri :calls}."
  (if (>= depth max-depth) []
  (begin
  (var rows (ox:query store
    (string/format "SELECT ?callee ?file ?line ?target WHERE {{
       <{}> <urn:rust:calls> ?target .
       ?target <urn:rust:name> ?callee .
       OPTIONAL {{ ?target <urn:rust:file> ?file }}
       OPTIONAL {{ ?target <urn:rust:line> ?line }}
     }}" rust-fn)))
  (var result @[])
  (each row in rows
    (var callee-name (rdf-val (get row :callee)))
    (var callee-file (when (get row :file) (rdf-val (get row :file))))
    (var callee-line (when (get row :line) (rdf-val (get row :line))))
    (var callee-iri (string/format "urn:rust:fn:{}" (rust-rdf:encode-name callee-name)))
    (var children (query-rust-callees callee-iri (+ depth 1) max-depth))
    (push result {:name callee-name :file callee-file :line callee-line
                  :iri callee-iri :calls children}))
  (freeze result))))

(defn format-rust-tree [nodes indent]
  "Render a Rust call tree as indented text with file:line and IRI."
  (var out @"")
  (each node in nodes
    (push out (string/format "{}[rust] {}" indent (get node :name)))
    (when (get node :file)
      (push out (string/format " {}:{}" (get node :file) (or (get node :line) "?"))))
    (push out (string/format "  <{}>\n" (get node :iri)))
    (push out (format-rust-tree (get node :calls) (string indent "  "))))
  (freeze out))

(defn call-trace [arguments]
  (var path (get arguments "path"))
  (var fn-name (get arguments "function"))
  (var max-depth (or (get arguments "depth") 2))
  (when (nil? path)
    (error {:error :invalid-params :message "missing required parameter: path"}))
  (when (nil? fn-name)
    (error {:error :invalid-params :message "missing required parameter: function"}))

  (var analysis (get-or-analyze path))
  (var callees (compile/callees analysis (keyword fn-name)))

  (var out @"")
  (push out (string/format "Trace: {} in {}\n\n" fn-name path))

  (each callee in callees
    (var callee-name (get callee :name))
    (var callee-line (or (get callee :line) "?"))
    (var callee-tail (or (get callee :tail) false))

    # Check if this callee is a primitive with a Rust implementation
    (var impl-rows (ox:query store
      (string/format "SELECT ?impl ?file ?line WHERE {{
         <urn:elle:fn:{}> <urn:elle:implemented-by> ?impl .
         OPTIONAL {{ ?impl <urn:rust:file> ?file }}
         OPTIONAL {{ ?impl <urn:rust:line> ?line }}
       }}" (rust-rdf:encode-name callee-name))))

    (var elle-fn-iri (string/format "urn:elle:fn:{}" (rust-rdf:encode-name callee-name)))
    (push out (string/format "  [elle] {} (line {}, tail={})  <{}>\n"
      callee-name callee-line callee-tail elle-fn-iri))

    (if (empty? impl-rows)
      # Not a primitive — it's an Elle-defined function, show its signal
      (let [[[ok? sig] (protect (compile/signal analysis (keyword callee-name)))]]
        (when ok?
          (push out (string/format "         signal: {}\n"
            (if (get sig :silent) "silent"
              (string/join (map string (->list (get sig :bits))) ", "))))))

      # Primitive — trace into Rust
      (each impl-row in impl-rows
        (var impl-iri (rdf-val (get impl-row :impl)))
        (var rust-file (when (get impl-row :file) (rdf-val (get impl-row :file))))
        (var rust-line (when (get impl-row :line) (rdf-val (get impl-row :line))))

        # Extract the Rust function name from the IRI
        (var rust-name-rows (ox:query store
          (string/format "SELECT ?name WHERE {{ <{}> <urn:rust:name> ?name }}" impl-iri)))
        (var rust-name (if (empty? rust-name-rows)
                         impl-iri
                         (rdf-val (get (first rust-name-rows) :name))))

        (push out (string/format "    -> [rust] {}" rust-name))
        (when rust-file
          (push out (string/format " {}:{}" rust-file (or rust-line "?"))))
        (push out (string/format "  <{}>\n" impl-iri))

        # Trace deeper into Rust calls
        (var children (query-rust-callees impl-iri 0 max-depth))
        (push out (format-rust-tree children "         ")))))

  (text-content (freeze out)))

# ── Tool dispatch ────────────────────────────────────────────────────────

(defn dispatch-tool [name arguments]
  (case name
    "ping"                (text-content "pong")
    "sparql_query"        (call-sparql-query arguments)
    "sparql_update"       (call-sparql-update arguments)
    "load_rdf"            (call-load-rdf arguments)
    "dump_rdf"            (call-dump-rdf arguments)
    "analyze_file"        (call-analyze-file arguments)
    "portrait"            (call-portrait arguments)
    "signal_query"        (call-signal-query arguments)
    "impact"              (call-impact arguments)
    "verify_invariants"   (call-verify-invariants arguments)
    "compile_rename"      (call-compile-rename arguments)
    "compile_extract"     (call-compile-extract arguments)
    "compile_parallelize" (call-compile-parallelize arguments)
    "trace"               (call-trace arguments)
    (error {:error :method-not-found
            :message (string/format "unknown tool: {}" name)})))

# ── MCP method dispatch ─────────────────────────────────────────────────

(defn handle-initialize [id _params]
  (jsonrpc-result id
    {:protocolVersion "2025-03-26"
     :capabilities {:tools {:listChanged false}}
     :serverInfo {:name "elle-mcp" :version "0.5.0"}
     :instructions "Elle semantic analysis server with RDF knowledge graph and program transformation. Use tools/list to discover available tools."}))

(defn handle-tools-list [id _params]
  (jsonrpc-result id {:tools all-tools}))

(defn handle-tools-call [id params]
  (let [[name (get params "name")]
        [arguments (or (get params "arguments") {})]]
    (let [[[ok? content] (protect (dispatch-tool name arguments))]]
      (if ok?
        (jsonrpc-result id {:content content})
        (jsonrpc-result id {:content (error-content
                                       (string/format "Internal error: {}"
                                         (get content :message)))
                            :isError true})))))

(defn handle-ping [id _params]
  (jsonrpc-result id {}))

(defn handle-request [msg]
  (let [[method (get msg "method")]
        [id     (get msg "id")]
        [params (or (get msg "params") {})]]
    (if (nil? id)
      nil
      (case method
        "initialize"  (handle-initialize id params)
        "ping"        (handle-ping id params)
        "tools/list"  (handle-tools-list id params)
        "tools/call"  (handle-tools-call id params)
        (jsonrpc-error id -32601 (string/format "method not found: {}" method))))))

# ── Main loop ────────────────────────────────────────────────────────────

(eprintln "elle-mcp server starting (v0.5.0)")
(eprintln "  store: " store-path)
(eprintln "  rust: " rust-file-count " files loaded")

# Watcher fiber
(eprintln "  watch: enabled")
(ev/spawn (fn []
  (var watcher (watch:start "." :filter ".lisp"))
  (watch:each watcher (fn [event]
    (let [[path (get event :path)]]
      (when (and (string/ends-with? path ".lisp")
                 (contains? |:create :modify| (get event :kind)))
        (let [[[ok? err] (protect
                (begin
                  (var old-analysis (get analysis-cache path))
                  (var old-sigs (when (not (nil? old-analysis))
                                  (snapshot-signals old-analysis)))
                  (invalidate-cache path)
                  (var new-analysis (get-or-analyze path))
                  (var new-sigs (snapshot-signals new-analysis))
                  (when (not (nil? old-sigs))
                    (var diff (diff-signals old-sigs new-sigs))
                    (when (or (not (empty? (get diff :added)))
                              (not (empty? (get diff :removed)))
                              (not (empty? (get diff :changed))))
                      (send-response (build-notification path diff))))
                  (eprintln "  re-analyzed: " path)))]]
          (unless ok?
            (eprintln "  watch error for " path ": " (string err))))))))))

(forever
  (let [[line (port/read-line (*stdin*))]]
    (when (nil? line)
      (eprintln "stdin closed, shutting down")
      (break))
    (unless (empty? line)
      (let [[[ok? msg] (protect (json/parse line))]]
        (if (not ok?)
          (begin
            (eprintln "JSON parse error: " (get msg :message))
            (send-response (jsonrpc-error nil -32700 "parse error")))
          (let [[response (handle-request msg)]]
            (unless (nil? response)
              (send-response response))))))))

) # end parameterize
