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

(def ox (import "plugin/oxigraph"))
(def glob ((import "std/glob")))
(def portrait-lib ((import "std/portrait")))
(def rdf ((import "std/rdf/elle")))

# syn plugin is optional — Rust graph features are disabled without it.
(var syn nil)
(var rust-rdf nil)
(let [[[ok? s] (protect (import "plugin/syn"))]]
  (when ok?
    (assign syn s)
    (assign rust-rdf ((import "std/rdf/rust") syn))))

# ── Watch + UUID libraries ──────────────────────────────────────────────

(def watch ((import "std/watch")))
(def uuid-lib ((import "std/uuid")))

# ── Store initialization ─────────────────────────────────────────────────

(def cli-args (drop 1 (sys/args)))

(def store-path
  (cond
    ((not (empty? cli-args))       (first cli-args))
    ((sys/env "ELLE_MCP_STORE")    (sys/env "ELLE_MCP_STORE"))
    (true                          ".elle-mcp/store")))

(defn nuke-store [path]
  "Delete a corrupt store directory so it can be recreated fresh.
   Refuses to delete if no oxigraph marker file (LOCK) is found."
  (let [[entries (glob:glob (string path "/*"))]]
    (when (not (empty? entries))
      (var has-marker false)
      (each e in entries
        (when (or (string/ends-with? e "/LOCK")
                  (string/ends-with? e "/CURRENT"))
          (assign has-marker true)))
      (unless has-marker
        (error {:error :safety
                :message (string "refusing to nuke " path
                          ": does not look like an oxigraph store")}))
      (each entry in entries
        (file/delete entry))))
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

(def rust-source-globs
  ["src/**/*.rs" "plugins/**/*.rs" "tests/**/*.rs"
   "benches/**/*.rs" "patches/**/*.rs"])

(defn populate-rust []
  "Parse source .rs files, load Rust triples and primitive cross-links.
   Yields between files so the main loop can process requests.
   Skips entirely when the syn plugin is not available."
  (if (nil? rust-rdf) 0
    (begin
      (var files @[])
      (each pattern in rust-source-globs
        (each f in (glob:glob pattern)
          (push files f)))
      (var count 0)
      (each file in files
        (when-ok [_ (begin
                      (ox:load store (rust-rdf:file file) :ntriples)
                      (ox:load store (rust-rdf:primitive-links file) :ntriples))]
          (assign count (inc count)))
        (yield nil))
      (flush-store)
      count)))

(defn clear-file-triples [path]
  "Remove all triples for a file from the RDF store."
  (ox:update store
    (string/format "DELETE WHERE {{ ?s <urn:elle:file> \"{}\" . ?s ?p ?o . }}"
      (string/replace path "\\" "\\\\"))))

(defn populate-file [analysis path]
  "Load triples for an analyzed file, replacing any existing.
   Delete-then-load is not atomic, but both are FFI calls (no yield),
   so the gap is crash-only — a crash between them loses triples for
   this file until the next analyze repopulates them."
  (clear-file-triples path)
  (ox:load store (rdf:file analysis path) :ntriples)
  (flush-store))

# ── Analysis cache ───────────────────────────────────────────────────────

(def analysis-cache @{})
(def analyzing-map @{})

(defn get-or-analyze [path]
  "Return cached analysis or analyze the file fresh.
   Guards against concurrent analysis of the same file — if another
   fiber is already analyzing this path, returns the stale cache entry."
  (var cached (get analysis-cache path))
  (if (not (nil? cached))
    cached
    (if (not (nil? (get analyzing-map path)))
      nil
      (begin
        (put analyzing-map path true)
        (defer (put analyzing-map path nil)
          (var result (compile/analyze (file/read path) {:file path}))
          (put analysis-cache path result)
          (populate-file result path)
          result)))))

(defn invalidate-cache [path]
  "Remove a path from the analysis cache so it is re-analyzed on next access."
  (put analysis-cache path nil))

# ── Eval handle table ───────────────────────────────────────────────────

(def eval-handles @{})

(defn handle-put [value]
  "Store a value in the handle table, return its UUID."
  (let [[id (uuid-lib:v4)]]
    (put eval-handles id {:value value})
    id))

(defn handle-get [id]
  "Retrieve a value by handle. Errors if the handle is unknown."
  (let [[entry (get eval-handles id)]]
    (when (nil? entry)
      (error {:error :unknown-handle :message (string "unknown handle: " id)}))
    (get entry :value)))

(defn value-kind [val is-error]
  "Kind string for a value. Reports :error when the eval failed."
  (if is-error ":error" (string ":" (type-of val))))

(defn value-shape [val]
  "Cheap shape hint: count for collections, keys_sample for structs."
  (case (type-of val)
    :array   {:count (length val)}
    :@array  {:count (length val)}
    :list    {:count (length val)}
    :struct  (let [[ks (keys val)]]
               {:count (length ks)
                :keys_sample (freeze (take 5 ks))})
    :@struct (let [[ks (keys val)]]
               {:count (length ks)
                :keys_sample (freeze (take 5 ks))})
    :string  {:bytes (length val)}
    :@string {:bytes (length val)}
    :set     {:count (length val)}
    :@set    {:count (length val)}
    nil))

# ── Signal diff (for watch notifications) ────────────────────────────────

(defn snapshot-signals [analysis]
  "Build a map of function-name -> signal struct for diff comparison."
  (var result @{})
  (each sym in (compile/symbols analysis)
    (when (= (get sym :kind) :function)
      (var name (get sym :name))
      (when-ok [sig (compile/signal analysis (keyword name))]
        (put result name sig))))
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
  (println (json/serialize response))
  (port/flush (*stdout*)))

(defn err-msg [e]
  "Safely extract a message from any error value.
   Works on structs (extracts :message), strings, and other types."
  (if (struct? e) (or (get e :message) (string e)) (string e)))

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

(def tool-eval
  {:name "eval"
   :description "Evaluate an Elle lambda against the persistent image. Returns a handle (UUID) naming the result — large values stay in the image. Compose by passing prior handles as inputs. stdout/stderr are captured and returned."
   :inputSchema {:type "object"
                 :properties {:lambda     {:type "string"
                                           :description "Elle source for a callable expression, e.g. \"(fn [prev] (take 10 prev))\""}
                              :inputs     {:type "array"
                                           :items {:type "string"}
                                           :description "Handles from prior eval calls, passed positionally as arguments"}
                              :timeout_ms {:type "integer"
                                           :description "Wall-clock timeout in milliseconds (default: 10000; 0 to disable)"}}
                 :required ["lambda"]}})

# ── SPARQL tool handlers ─────────────────────────────────────────────────

(defn call-sparql-query [arguments]
  (let [[query (get arguments "query")]]
    (when (nil? query)
      (error {:error :invalid-params :message "missing required parameter: query"}))
    (let [[[ok? result] (protect (ev/timeout 30 (fn [] (ox:query store query))))]]
      (if ok?
        (text-content (cond
          ((boolean? result) (if result "true" "false"))
          ((array? result)   (if (empty? result) "No results." (json/pretty result)))
          (true              (string result))))
        (error-content (string/format "SPARQL error: {}" (err-msg result)))))))

(defn call-sparql-update [arguments]
  (let [[update-str (get arguments "update")]]
    (when (nil? update-str)
      (error {:error :invalid-params :message "missing required parameter: update"}))
    (let [[[ok? result] (protect (ox:update store update-str))]]
      (if ok?
        (begin (flush-store) (text-content "Update executed successfully."))
        (error-content (string/format "SPARQL update error: {}" (err-msg result)))))))

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
        (error-content (string/format "Load error: {}" (err-msg result)))))))

(defn call-dump-rdf [arguments]
  (let* [[fmt (or (get arguments "format") "turtle")]
         [[ok? result] (protect (ox:dump store (keyword fmt)))]]
    (if ok?
      (text-content result)
      (error-content (string/format "Dump error: {}" (err-msg result))))))

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
    (when-ok [sig (compile/signal analysis (keyword name))]
      (cond
        ((get sig :silent)                         (push silent-names name))
        ((not (empty? (get sig :propagates)))      (push delegating-names name))
        ((get sig :io)                             (push io-names name))
        ((get sig :yields)                         (push yielding-names name))
        (true                                      (push io-names name)))

      (when-ok [caps (compile/captures analysis (keyword name))]
        (when-ok [callees (compile/callees analysis (keyword name))]
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
    (push out (string/format "  {} (line {}, tail={})"
      caller-name (or (get c :line) "?") (or (get c :tail) false)))
    (when-ok [caller-sig (compile/signal analysis (keyword caller-name))]
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
              :message (string/format "cannot parse invariants: {}" (err-msg result))})))

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
          (push out (string/format "  x {} — query error: {}\n" name (err-msg result)))
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
  (when (nil? rust-rdf)
    (error {:error :unavailable
            :message "trace requires the syn plugin (plugin/syn not found)"}))
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
      (when-ok [sig (compile/signal analysis (keyword callee-name))]
        (push out (string/format "         signal: {}\n"
          (if (get sig :silent) "silent"
            (string/join (map string (->list (get sig :bits))) ", ")))))

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

# ── Eval tool handler ──────────────────────────────────────────────────

(defn call-eval [arguments]
  (let* [[lambda-src (get arguments "lambda")]
         [input-ids  (or (get arguments "inputs") [])]
         [timeout-ms (or (get arguments "timeout_ms") 10000)]]

    (when (nil? lambda-src)
      (error {:error :invalid-params :message "missing required parameter: lambda"}))

    # Parse the lambda source
    (var parsed nil)
    (let [[[ok? val] (protect (read lambda-src))]]
      (if ok? (assign parsed val)
        (error {:error :parse-error
                :message (string "cannot parse lambda: " (err-msg val))})))

    # Eval to get a callable value
    (var callable nil)
    (let [[[ok? val] (protect (eval parsed))]]
      (if ok? (assign callable val)
        (error {:error :eval-error
                :message (string "lambda eval failed: " (err-msg val))})))

    (when (not (callable? callable))
      (error {:error :type-error
              :message (string "lambda must evaluate to a callable, got "
                         (type-of callable))}))

    # Resolve input handles
    (var input-vals @[])
    (each id in input-ids
      (push input-vals (handle-get id)))

    # Arity check for closures
    (when (= (type-of callable) :closure)
      (let [[a (arity callable)]
            [n (length input-ids)]]
        (cond
          ((nil? a)     nil)
          ((integer? a) (unless (= a n)
                          (error {:error :arity-mismatch
                                  :message (string "lambda expects " a " args, got " n)})))
          (true         (unless (>= n (first a))
                          (error {:error :arity-mismatch
                                  :message (string "lambda expects at least "
                                             (first a) " args, got " n)}))))))

    # Set up temp files for I/O capture
    (file/mkdir-all ".elle-mcp")
    (let* [[eval-id (uuid-lib:v4)]
           [out-path (string ".elle-mcp/eval-" eval-id "-out")]
           [err-path (string ".elle-mcp/eval-" eval-id "-err")]
           [out-port (port/open out-path :write)]
           [err-port (port/open err-path :write)]]
      (defer (begin
               (protect (file/delete out-path))
               (protect (file/delete err-path)))

        # Execute with I/O capture and optional timeout
        (let* [[input-list (freeze input-vals)]
               [thunk (fn []
                        (parameterize ((*stdout* out-port) (*stderr* err-port))
                          (apply callable input-list)))]
               [start (clock/monotonic)]
               [[ok? result] (if (and timeout-ms (> timeout-ms 0))
                               (protect (ev/timeout (/ timeout-ms 1000) thunk))
                               (protect (thunk)))]
               [duration-ns (int (* (- (clock/monotonic) start) 1000000000))]]

          # Flush and close captured I/O ports
          (protect (port/flush out-port))
          (protect (port/flush err-port))
          (protect (port/close out-port))
          (protect (port/close err-port))

          # Read captured output
          (var stdout-text "")
          (var stderr-text "")
          (let [[[rd-ok? rd-val] (protect (file/read out-path))]]
            (when rd-ok? (assign stdout-text rd-val)))
          (let [[[rd-ok? rd-val] (protect (file/read err-path))]]
            (when rd-ok? (assign stderr-text rd-val)))

          # Build response
          (let* [[handle (handle-put result)]
                 [kind (value-kind result (not ok?))]
                 [shape (if ok?
                          (value-shape result)
                          {:reason (get result :error)
                           :message (err-msg result)})]]
            (text-content (json/serialize
              {:ok ok?
               :handle handle
               :kind kind
               :shape shape
               :stdout stdout-text
               :stderr stderr-text
               :duration_ns duration-ns
               :fibers []}))))))))

# ── Test orchestration ───────────────────────────────────────────────────

(def tool-test-run
  {:name "test_run"
   :description "Run tests and record results. Captures exit code, duration, stdout/stderr. Records result keyed by (sha, mode) in the RDF store."
   :inputSchema {:type "object"
                 :properties {:path {:type "string" :description "Specific test file (optional)"}
                              :mode {:type "string" :enum ["smoke" "test" "single"]
                                     :description "Test scope: smoke (~30s), test (~3min), or single file"}
                              :jit {:type "string" :enum ["off" "eager" "adaptive"]
                                    :description "Override JIT policy (optional)"}}
                 :required ["mode"]}})

(def tool-test-status
  {:name "test_status"
   :description "Query test results for a commit. Returns structured summary: passed/failed count, failure details with location and context. Agents never need to re-run tests with | tail to read output."
   :inputSchema {:type "object"
                 :properties {:sha {:type "string" :description "Git SHA (default: HEAD)"}
                              :mode {:type "string" :description "Filter by mode: smoke or test"}}
                 :required []}})

(def tool-test-history
  {:name "test_history"
   :description "Test results across recent commits."
   :inputSchema {:type "object"
                 :properties {:path {:type "string" :description "Specific test file (optional)"}
                              :limit {:type "integer" :description "How many commits back (default: 10)"}}
                 :required []}})

(def tool-test-gate
  {:name "test_gate"
   :description "Check if SHA is clear to push. Verifies a full make test pass exists for the SHA on a clean worktree."
   :inputSchema {:type "object"
                 :properties {:sha {:type "string" :description "Git SHA (default: HEAD)"}}
                 :required []}})

(def tool-push-ready
  {:name "push_ready"
   :description "Push with test gate. Checks test_gate for HEAD, pushes if passing."
   :inputSchema {:type "object"
                 :properties {:remote {:type "string" :description "Remote name (default: origin)"}
                              :branch {:type "string" :description "Branch name"}}
                 :required ["branch"]}})

(def tool-push-wip
  {:name "push_wip"
   :description "Push without test gate. For saving work or requesting review."
   :inputSchema {:type "object"
                 :properties {:remote {:type "string" :description "Remote name (default: origin)"}
                              :branch {:type "string" :description "Branch name"}}
                 :required ["branch"]}})

# ── Aggregate all tools ──────────────────────────────────────────────────
#
# Must come after every (def tool-* ...) form: the surrounding
# (parameterize ...) body is analyzed sequentially, not as a letrec,
# so forward references to tools defined later would fail with
# "undefined variable: tool-..." (surfacing as a poison node at the
# array-literal expansion site).

(def all-tools
  [tool-ping tool-sparql-query tool-sparql-update tool-load-rdf tool-dump-rdf
   tool-analyze-file tool-portrait tool-signal-query tool-impact
   tool-verify-invariants tool-compile-rename tool-compile-extract
   tool-compile-parallelize tool-trace tool-eval
   tool-test-run tool-test-status tool-test-history tool-test-gate
   tool-push-ready tool-push-wip])

# ── Test orchestration handlers ──────────────────────────────────────────

(defn git-sha []
  "Get current HEAD SHA."
  (let [[proc (subprocess/exec "git" @["rev-parse" "HEAD"])]]
    (string/trim (port/read-all (get proc :stdout)))))

(defn git-clean? []
  "Check if worktree is clean."
  (let [[proc (subprocess/exec "git" @["status" "--porcelain"])]]
    (empty? (string/trim (port/read-all (get proc :stdout))))))

(defn run-make-target [target jit-override]
  "Run a make target, return {:exit-code :stdout :stderr :duration}."
  (let* [[start (clock/monotonic)]
         [env-args (if jit-override
                     {:env {:ELLE_JIT jit-override}}
                     {})]
         [proc (subprocess/exec "make" @[target] env-args)]
         [stdout (port/read-all (get proc :stdout))]
         [stderr (port/read-all (get proc :stderr))]
         [status (subprocess/wait proc)]
         [duration (- (clock/monotonic) start)]]
    {:exit-code (get status :exit-code)
     :stdout stdout
     :stderr stderr
     :duration (/ duration 1000000000)}))

(defn parse-test-failures [stderr]
  "Extract structured failure info from test stderr."
  (var failures @[])
  (each line in (string/split stderr "\n")
    (when (string/find line "✗")
      (push failures {:message (string/trim line)})))
  (freeze failures))

(defn turtle-escape [s]
  "Escape a string for embedding in a Turtle literal."
  (var esc (string/replace s "\\" "\\\\"))
  (assign esc (string/replace esc "\"" "\\\""))
  (assign esc (string/replace esc "\n" "\\n"))
  (assign esc (string/replace esc "\r" "\\r"))
  esc)

(defn store-test-result [sha mode clean passed duration failures stderr]
  "Store test result as RDF triples, including truncated stderr."
  (let* [[iri (string/format "urn:test:{}:{}" sha mode)]
         [timestamp (clock/realtime)]
         [trunc-stderr (if (> (length stderr) 10000)
                         (string (slice stderr 0 10000) "\n...[truncated]")
                         stderr)]
         [ttl (string/format
               "<{}> a <urn:elle:TestRun> ;
                   <urn:elle:sha> \"{}\" ;
                   <urn:elle:mode> \"{}\" ;
                   <urn:elle:clean> {} ;
                   <urn:elle:passed> {} ;
                   <urn:elle:duration> {} ;
                   <urn:elle:timestamp> \"{}\" ;
                   <urn:elle:failed-count> {} ;
                   <urn:elle:stderr> \"{}\" ."
               iri sha mode
               (if clean "true" "false")
               (if passed "true" "false")
               duration timestamp
               (length failures)
               (turtle-escape trunc-stderr))]]
    # Delete old result for this sha+mode, then insert new.
    # Both are FFI calls (no yield), so the gap is crash-only.
    (protect (ox:update store
      (string/format "DELETE WHERE {{ <{}> ?p ?o . }}" iri)))
    (protect (ox:load store ttl :turtle))
    (flush-store)))

(defn call-test-run [arguments]
  (let* [[mode (get arguments "mode")]
         [jit-override (get arguments "jit")]
         [sha (git-sha)]
         [clean (git-clean?)]
         [target (case mode
                   "smoke" "smoke"
                   "test"  "test"
                   "single" (let [[path (get arguments "path")]]
                              (when (nil? path)
                                (error {:error :invalid-params
                                        :message "single mode requires path parameter"}))
                              nil)
                   (error {:error :invalid-params
                           :message (string/format "unknown mode: {}" mode)}))]
         [result (if target
                   (run-make-target target jit-override)
                   (let [[path (get arguments "path")]
                         [elle-bin (or (sys/env "ELLE") "./target/debug/elle")]
                         [jit-flag (case jit-override
                                     "off" "--jit=0"
                                     "eager" "--jit=1"
                                     nil "")]]
                     (let* [[args (if (empty? jit-flag) @[path] @[jit-flag path])]
                            [proc (subprocess/exec elle-bin args)]
                            [stdout (port/read-all (get proc :stdout))]
                            [stderr (port/read-all (get proc :stderr))]
                            [status (subprocess/wait proc)]]
                       {:exit-code (get status :exit-code)
                        :stdout stdout :stderr stderr :duration 0})))]
         [passed (= (get result :exit-code) 0)]
         [failures (if passed () (parse-test-failures (get result :stderr)))]]
    (store-test-result sha mode clean passed (get result :duration) failures
                       (get result :stderr))
    (text-content (json/pretty
      {:passed passed
       :failed-count (length failures)
       :failures failures
       :duration (get result :duration)
       :clean clean
       :sha sha}))))

(defn query-test-result [sha mode]
  "Query stored test result for sha+mode."
  (let [[query (string/format
                 "SELECT ?passed ?clean ?duration ?failed_count ?timestamp
                  WHERE {{
                    <urn:test:{}:{}> <urn:elle:passed> ?passed ;
                                     <urn:elle:clean> ?clean ;
                                     <urn:elle:duration> ?duration ;
                                     <urn:elle:failed-count> ?failed_count ;
                                     <urn:elle:timestamp> ?timestamp .
                  }}" sha mode)]]
    (let [[[ok? rows] (protect (ox:query store query))]]
      (if (and ok? (not (empty? rows)))
        (first rows)
        nil))))

(defn call-test-status [arguments]
  (let* [[sha (or (get arguments "sha") (git-sha))]
         [mode (get arguments "mode")]
         [modes (if mode (list mode) (list "smoke" "test"))]]
    (var results @[])
    (each m in modes
      (let [[r (query-test-result sha m)]]
        (when r (push results (put r "mode" m)))))
    (if (empty? results)
      (text-content (json/pretty {:sha sha :results "no test records found"}))
      (text-content (json/pretty {:sha sha :results (freeze results)})))))

(defn call-test-history [arguments]
  (let* [[limit (or (get arguments "limit") 10)]
         [query (string/format
                  "SELECT ?sha ?mode ?passed ?clean ?duration ?timestamp
                   WHERE {{
                     ?run a <urn:elle:TestRun> ;
                          <urn:elle:sha> ?sha ;
                          <urn:elle:mode> ?mode ;
                          <urn:elle:passed> ?passed ;
                          <urn:elle:clean> ?clean ;
                          <urn:elle:duration> ?duration ;
                          <urn:elle:timestamp> ?timestamp .
                   }}
                   ORDER BY DESC(?timestamp)
                   LIMIT {}" limit)]]
    (let [[[ok? rows] (protect (ox:query store query))]]
      (if ok?
        (text-content (json/pretty rows))
        (error-content "Failed to query test history")))))

(defn call-test-gate [arguments]
  (let* [[sha (or (get arguments "sha") (git-sha))]
         [result (query-test-result sha "test")]]
    (if (nil? result)
      (text-content (json/pretty {:ready false :reason "no test record for this SHA"}))
      (let [[passed (get result "passed")]
            [clean  (get result "clean")]]
        (cond
          ((not passed)
           (text-content (json/pretty {:ready false :reason "tests failed"})))
          ((not clean)
           (text-content (json/pretty {:ready false :reason "worktree was dirty when tests ran"})))
          (true
           (text-content (json/pretty {:ready true :sha sha}))))))))

(defn call-push [arguments gated]
  (let* [[remote (or (get arguments "remote") "origin")]
         [branch (get arguments "branch")]]
    (when (nil? branch)
      (error {:error :invalid-params :message "missing required parameter: branch"}))
    (when gated
      (let* [[sha (git-sha)]
             [result (query-test-result sha "test")]]
        (when (or (nil? result) (not (get result "passed")) (not (get result "clean")))
          (let [[reason (cond
                          ((nil? result) "no test record for HEAD")
                          ((not (get result "passed")) "tests failed")
                          (true "worktree was dirty"))]]
            (error {:error :test-gate-failed
                    :message (string/format "push blocked: {}" reason)})))))
    (let* [[proc (subprocess/exec "git" @["push" remote branch])]
           [stderr (port/read-all (get proc :stderr))]
           [status (subprocess/wait proc)]]
      (if (= (get status :exit-code) 0)
        (text-content (string/format "pushed {} to {}/{}" (git-sha) remote branch))
        (error-content (string/format "push failed: {}" stderr))))))

(defn call-push-ready [arguments]
  (call-push arguments true))

(defn call-push-wip [arguments]
  (call-push arguments false))

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
    "eval"                (call-eval arguments)
    "test_run"            (call-test-run arguments)
    "test_status"         (call-test-status arguments)
    "test_history"        (call-test-history arguments)
    "test_gate"           (call-test-gate arguments)
    "push_ready"          (call-push-ready arguments)
    "push_wip"            (call-push-wip arguments)
    (error {:error :method-not-found
            :message (string/format "unknown tool: {}" name)})))

# ── MCP method dispatch ─────────────────────────────────────────────────

(defn handle-initialize [id _params]
  (jsonrpc-result id
    {:protocolVersion "2025-03-26"
     :capabilities {:tools {:listChanged false}}
     :serverInfo {:name "elle-mcp" :version "0.6.0"}
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
                                         (err-msg content)))
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

(eprintln "elle-mcp server starting (v0.6.0)")
(eprintln "  store: " store-path)

# Population fiber — yields between work units so the main loop
# can process requests between FFI calls.
(populate-primitives)
(eprintln "  primitives: loaded")

(def populator (fiber/new (fn []
  (var count (populate-rust))
  (eprintln "  rust: " count " files loaded")
  (send-response {:jsonrpc "2.0"
                  :method "notifications/model/populated"
                  :params {:primitives true :rust count}}))
  |:yield|))

# Initial resume — starts populate-rust, runs until first (yield nil)
(fiber/resume populator nil)

(defn tick-populator []
  "Resume the population fiber one step if it's still alive."
  (when (= (fiber/status populator) :paused)
    (fiber/resume populator nil)))

# Watcher fiber — restarts on crash, capped at 5 attempts.
(eprintln "  watch: enabled")
(ev/spawn (fn []
  (var restarts 0)
  (while (< restarts 5)
    (let [[[ok? err] (protect
            (begin
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
                        (eprintln "  watch error for " path ": " (err-msg err))))))))))]]
      (unless ok?
        (eprintln "  watcher crashed (" (inc restarts) "/5): " (err-msg err))
        (assign restarts (inc restarts))
        (ev/sleep 1))))))

(forever
  (let [[line (port/read-line (*stdin*))]]
    (when (nil? line)
      (eprintln "stdin closed, shutting down")
      (break))
    (tick-populator)
    (if (> (length line) 10000000)
      (send-response (jsonrpc-error nil -32600 "request too large"))
      (unless (empty? line)
        (let [[[ok? msg] (protect (json/parse line))]]
          (if (not ok?)
            (begin
              (eprintln "JSON parse error: " (err-msg msg))
              (send-response (jsonrpc-error nil -32700 "parse error")))
            (let [[response (handle-request msg)]]
              (unless (nil? response)
                (send-response response)))))))))

) # end parameterize
