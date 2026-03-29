#!/usr/bin/env elle
## load-all.lisp — extract Elle + Rust graphs and load into oxigraph store
##
## Runs both extractors and loads their ntriples directly into an
## oxigraph store (no MCP server needed).
##
## Run:  elle tools/load-all.lisp
(elle/epoch 5)

(def glob-plugin (import "target/release/libelle_glob.so"))
(def do-glob (get glob-plugin :glob))

(def syn-plugin (import "target/release/libelle_syn.so"))
(def syn-parse-file     (get syn-plugin :parse-file))
(def syn-items          (get syn-plugin :items))
(def syn-item-kind      (get syn-plugin :item-kind))
(def syn-item-name      (get syn-plugin :item-name))
(def syn-fn-info        (get syn-plugin :fn-info))
(def syn-fn-calls       (get syn-plugin :fn-calls))
(def syn-primitive-defs (get syn-plugin :primitive-defs))
(def syn-struct-fields  (get syn-plugin :struct-fields))
(def syn-enum-variants  (get syn-plugin :enum-variants))
(def syn-visibility     (get syn-plugin :visibility))
(def syn-attributes     (get syn-plugin :attributes))
(def syn-to-string      (get syn-plugin :to-string))

(def ox (import "target/release/libelle_oxigraph.so"))

# ── Store ────────────────────────────────────────────────────────────

(def store-path ".elle-mcp/store")
(file/mkdir-all store-path)
(def store (ox:store-open store-path))

# ── Triple helpers ───────────────────────────────────────────────────

(defn nt-iri [s]
  (string/format "<{}>" s))

(defn nt-encode [name]
  (-> name
    (string/replace "%" "%25")
    (string/replace " " "%20")
    (string/replace "*" "%2A")
    (string/replace ">" "%3E")
    (string/replace "<" "%3C")
    (string/replace "?" "%3F")
    (string/replace "#" "%23")
    (string/replace "!" "%21")
    (string/replace "'" "%27")
    (string/replace "[" "%5B")
    (string/replace "]" "%5D")
    (string/replace "(" "%28")
    (string/replace ")" "%29")
    (string/replace "{" "%7B")
    (string/replace "}" "%7D")
    (string/replace "\"" "%22")
    (string/replace "," "%2C")
    (string/replace ";" "%3B")
    (string/replace "`" "%60")
    (string/replace "@" "%40")
    (string/replace "+" "%2B")
    (string/replace "=" "%3D")
    (string/replace "|" "%7C")
    (string/replace "\\" "%5C")
    (string/replace "^" "%5E")))

(defn nt-lit [s]
  (let [[escaped (-> s
                   (string/replace "\\" "\\\\")
                   (string/replace "\"" "\\\"")
                   (string/replace "\n" "\\n"))]]
    (string/format "\"{}\"" escaped)))

(defn nt-triple [buf s p o]
  (push buf (string/format "{} {} {} .\n" s p o)))

(def rdf-type (nt-iri "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"))

# ── Elle extractor ───────────────────────────────────────────────────

(defn extract-elle-file [buf file]
  (let [[[ok? src] (protect (file/read file))]]
    (when ok?
      (let [[[ok? forms] (protect (read-all src))]]
        (when ok?
          (each form in forms
            (when (pair? form)
              (let [[head (string (first form))]
                    [ns "urn:elle"]]
                (case head
                  "defn"
                  (when (>= (length form) 3)
                    (let* [[parts (drop 1 form)]
                           [name (string (first parts))]
                           [params-form (get parts 1)]
                           [subj (nt-iri (string/format "{}:fn:{}" ns (nt-encode name)))]
                           [param-names (map (fn [p] (string p))
                                             (if (array? params-form)
                                               (filter (fn [p] (not (= (string p) "&")))
                                                       params-form)
                                               ()))]]
                      (nt-triple buf subj rdf-type (nt-iri (string/format "{}:Fn" ns)))
                      (nt-triple buf subj (nt-iri (string/format "{}:name" ns)) (nt-lit name))
                      (nt-triple buf subj (nt-iri (string/format "{}:file" ns)) (nt-lit file))
                      (nt-triple buf subj (nt-iri (string/format "{}:arity" ns)) (nt-lit (string (length param-names))))
                      (each p in param-names
                        (nt-triple buf subj (nt-iri (string/format "{}:param" ns)) (nt-lit p)))
                      (when (>= (length parts) 4)
                        (let [[maybe-doc (get parts 2)]]
                          (when (string? maybe-doc)
                            (nt-triple buf subj (nt-iri (string/format "{}:doc" ns)) (nt-lit maybe-doc)))))))

                  "defmacro"
                  (when (>= (length form) 3)
                    (let* [[parts (drop 1 form)]
                           [name (string (first parts))]
                           [subj (nt-iri (string/format "{}:macro:{}" ns (nt-encode name)))]]
                      (nt-triple buf subj rdf-type (nt-iri (string/format "{}:Macro" ns)))
                      (nt-triple buf subj (nt-iri (string/format "{}:name" ns)) (nt-lit name))
                      (nt-triple buf subj (nt-iri (string/format "{}:file" ns)) (nt-lit file))))

                  "def"
                  (when (>= (length form) 2)
                    (let [[name-form (get (drop 1 form) 0)]]
                      (when (symbol? name-form)
                        (let* [[name (string name-form)]
                               [subj (nt-iri (string/format "{}:def:{}" ns (nt-encode name)))]]
                          (when (>= (length form) 3)
                            (let [[val-form (get (drop 1 form) 1)]]
                              (when (and (pair? val-form) (= (string (first val-form)) "import"))
                                (let* [[path (string (get (drop 1 val-form) 0))]
                                       [isubj (nt-iri (string/format "{}:import:{}" ns (nt-encode name)))]]
                                  (nt-triple buf isubj rdf-type (nt-iri (string/format "{}:Import" ns)))
                                  (nt-triple buf isubj (nt-iri (string/format "{}:name" ns)) (nt-lit name))
                                  (nt-triple buf isubj (nt-iri (string/format "{}:path" ns)) (nt-lit path))
                                  (nt-triple buf isubj (nt-iri (string/format "{}:file" ns)) (nt-lit file))))))
                          (nt-triple buf subj rdf-type (nt-iri (string/format "{}:Def" ns)))
                          (nt-triple buf subj (nt-iri (string/format "{}:name" ns)) (nt-lit name))
                          (nt-triple buf subj (nt-iri (string/format "{}:file" ns)) (nt-lit file))))))

                  nil)))))))))

# ── Rust extractor ───────────────────────────────────────────────────

(defn extract-rust-file [buf file]
  (let [[[ok? src] (protect (file/read file))]]
    (when ok?
      (let [[[ok? tree] (protect (syn-parse-file src))]]
        (if (not ok?)
          (eprintln "warning: parse error in " file ": " (get tree :message))
          (each item in (syn-items tree)
            (let [[kind (syn-item-kind item)]
                  [ns "urn:rust"]]

              (defn rust-subj [kind-str name]
                (nt-iri (string/format "{}:{}:{}" ns kind-str (nt-encode (string name)))))

              (defn emit-vis [subj item]
                (nt-triple buf subj (nt-iri (string/format "{}:visibility" ns))
                           (nt-lit (string (syn-visibility item)))))

              (defn emit-attrs [subj item]
                (each attr in (syn-attributes item)
                  (nt-triple buf subj (nt-iri (string/format "{}:attribute" ns)) (nt-lit attr))))

              (defn emit-named [kind-str item]
                (let* [[name (syn-item-name item)]
                       [subj (rust-subj kind-str name)]]
                  (nt-triple buf subj rdf-type (nt-iri (string/format "{}:{}" ns kind-str)))
                  (nt-triple buf subj (nt-iri (string/format "{}:name" ns)) (nt-lit name))
                  (nt-triple buf subj (nt-iri (string/format "{}:file" ns)) (nt-lit file))
                  (emit-vis subj item)
                  (emit-attrs subj item)))

              (case kind
                :fn
                (let* [[info (syn-fn-info item)]
                       [name (get info :name)]
                       [subj (rust-subj "fn" name)]]
                  (nt-triple buf subj rdf-type (nt-iri (string/format "{}:Fn" ns)))
                  (nt-triple buf subj (nt-iri (string/format "{}:name" ns)) (nt-lit name))
                  (nt-triple buf subj (nt-iri (string/format "{}:file" ns)) (nt-lit file))
                  (each arg in (get info :args)
                    (nt-triple buf subj (nt-iri (string/format "{}:param" ns)) (nt-lit (get arg :name)))
                    (nt-triple buf subj (nt-iri (string/format "{}:param-type" ns))
                               (nt-lit (string/format "{}:{}" (get arg :name) (get arg :type)))))
                  (when (get info :return-type)
                    (nt-triple buf subj (nt-iri (string/format "{}:return-type" ns)) (nt-lit (get info :return-type))))
                  (when (get info :async?)
                    (nt-triple buf subj (nt-iri (string/format "{}:async" ns)) (nt-lit "true")))
                  (when (get info :unsafe?)
                    (nt-triple buf subj (nt-iri (string/format "{}:unsafe" ns)) (nt-lit "true")))
                  (emit-vis subj item)
                  (emit-attrs subj item)
                  # Emit call edges from function body.
                  (let [[[ok? calls] (protect (syn-fn-calls item))]]
                    (when ok?
                      (each callee in calls
                        (nt-triple buf subj
                                   (nt-iri (string/format "{}:calls" ns))
                                   (rust-subj "fn" callee))))))

                :struct
                (let* [[info (syn-struct-fields item)]
                       [name (get info :name)]
                       [subj (rust-subj "struct" name)]]
                  (nt-triple buf subj rdf-type (nt-iri (string/format "{}:Struct" ns)))
                  (nt-triple buf subj (nt-iri (string/format "{}:name" ns)) (nt-lit name))
                  (nt-triple buf subj (nt-iri (string/format "{}:file" ns)) (nt-lit file))
                  (nt-triple buf subj (nt-iri (string/format "{}:kind" ns)) (nt-lit (string (get info :kind))))
                  (each field in (get info :fields)
                    (when (get field :name)
                      (nt-triple buf subj (nt-iri (string/format "{}:field" ns)) (nt-lit (get field :name)))
                      (nt-triple buf subj (nt-iri (string/format "{}:field-type" ns))
                                 (nt-lit (string/format "{}:{}" (get field :name) (get field :type))))))
                  (emit-vis subj item)
                  (emit-attrs subj item))

                :enum
                (let* [[info (syn-enum-variants item)]
                       [name (get info :name)]
                       [subj (rust-subj "enum" name)]]
                  (nt-triple buf subj rdf-type (nt-iri (string/format "{}:Enum" ns)))
                  (nt-triple buf subj (nt-iri (string/format "{}:name" ns)) (nt-lit name))
                  (nt-triple buf subj (nt-iri (string/format "{}:file" ns)) (nt-lit file))
                  (each variant in (get info :variants)
                    (nt-triple buf subj (nt-iri (string/format "{}:variant" ns)) (nt-lit (get variant :name))))
                  (emit-vis subj item)
                  (emit-attrs subj item))

                :trait  (emit-named "Trait" item)
                :const  (emit-named "Const" item)
                :static (emit-named "Static" item)
                :type   (emit-named "Type" item)
                :mod    (emit-named "Mod" item)
                :use
                (let* [[path (syn-to-string item)]
                       [subj (nt-iri (string/format "{}:use:{}:{}" ns (nt-encode file) (nt-encode path)))]]
                  (nt-triple buf subj rdf-type (nt-iri (string/format "{}:Use" ns)))
                  (nt-triple buf subj (nt-iri (string/format "{}:path" ns)) (nt-lit path))
                  (nt-triple buf subj (nt-iri (string/format "{}:file" ns)) (nt-lit file))
                  (emit-vis subj item))

                :const
                (begin
                  (emit-named "Const" item)
                  # Extract primitive name→func mappings from PRIMITIVES tables.
                  (when (= (syn-item-name item) "PRIMITIVES")
                    (let [[[ok? defs] (protect (syn-primitive-defs item))]]
                      (when ok?
                        (each def in defs
                          (var elle-name (get def :name))
                          (var rust-fn   (get def :func))
                          (nt-triple buf
                                     (nt-iri (string/format "urn:elle:fn:{}" (nt-encode elle-name)))
                                     (nt-iri "urn:elle:implemented-by")
                                     (rust-subj "fn" rust-fn))
                          (nt-triple buf
                                     (rust-subj "fn" rust-fn)
                                     (nt-iri (string/format "{}:implements" ns))
                                     (nt-iri (string/format "urn:elle:fn:{}" (nt-encode elle-name)))))))))

                nil))))))))

# ── Main ─────────────────────────────────────────────────────────────

# Extract Elle
(eprintln "extracting Elle definitions...")
(def elle-buf @"")
(def elle-files (map (fn [f] f) (do-glob "**/*.lisp")))
(each file in elle-files
  (extract-elle-file elle-buf file))
(def elle-triples (freeze elle-buf))
(eprintln "  " (length elle-files) " .lisp files")

(eprintln "loading Elle triples...")
(ox:load store elle-triples :ntriples)
(eprintln "  done")

# Extract Rust
(eprintln "extracting Rust definitions...")
(def rust-buf @"")
(def rust-files (map (fn [f] f) (do-glob "**/*.rs")))
(each file in rust-files
  (extract-rust-file rust-buf file))
(def rust-triples (freeze rust-buf))
(eprintln "  " (length rust-files) " .rs files")

(eprintln "loading Rust triples...")
(ox:load store rust-triples :ntriples)
(eprintln "  done")

# Flush to disk so data survives process exit.
(eprintln "flushing store...")
(ox:store-flush store)
(eprintln "  done")

# Verify
(eprintln "")
(eprintln "verifying...")

# Helper to extract the value from an RDF term like [:literal "x" ...] or [:iri "x"]
(defn rdf-val [term]
  (get term 1))

(def total (ox:query store "SELECT (COUNT(*) AS ?count) WHERE { ?s ?p ?o }"))
(eprintln "  total triples: " (rdf-val (get (get total 0) :count)))

(def types (ox:query store "SELECT ?type (COUNT(?s) AS ?count)
WHERE { ?s <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> ?type }
GROUP BY ?type ORDER BY DESC(?count)"))
(eprintln "  types:")
(each row in types
  (eprintln "    " (rdf-val (get row :type)) "  " (rdf-val (get row :count))))

(def unsafe-fns (ox:query store "SELECT ?name ?file
WHERE {
  ?s <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <urn:rust:Fn> .
  ?s <urn:rust:name> ?name .
  ?s <urn:rust:file> ?file .
  ?s <urn:rust:unsafe> \"true\" .
}
LIMIT 5"))
(eprintln "")
(eprintln "  sample unsafe Rust fns:")
(each row in unsafe-fns
  (eprintln "    " (rdf-val (get row :name)) " in " (rdf-val (get row :file))))

(def elle-fns (ox:query store "SELECT ?name ?doc
WHERE {
  ?s <http://www.w3.org/1999/02/22-rdf-syntax-ns#type> <urn:elle:Fn> .
  ?s <urn:elle:name> ?name .
  ?s <urn:elle:doc> ?doc .
}
LIMIT 5"))
(eprintln "")
(eprintln "  sample Elle fns with docs:")
(each row in elle-fns
  (eprintln "    " (rdf-val (get row :name)) ": " (rdf-val (get row :doc))))

(eprintln "")
(eprintln "done. store at " store-path)
