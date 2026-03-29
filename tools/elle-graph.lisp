#!/usr/bin/env elle
## elle-graph.lisp — extract RDF triples from Elle source files
##
## Reads .lisp files, parses them with read-all, and emits ntriples
## describing top-level definitions (def, defn, defmacro, import).
##
## Usage:
##   elle elle-graph.lisp -- src.lisp lib.lisp ...
##   elle elle-graph.lisp                          # defaults to *.lisp in CWD

(elle/epoch 5)
(def glob-plugin (import "glob"))
(def do-glob (get glob-plugin :glob))

(def args (drop 1 (sys/args)))

# map to list — workaround for VM bug (see bug-repro.lisp)
(def files
  (map (fn [f] f)
       (if (empty? args) (do-glob "*.lisp") args)))

# ── Triple emitter ───────────────────────────────────────────────────

(def out @"")

(defn iri [s]
  (string/format "<{}>" s))

(defn encode-name [name]
  "Percent-encode chars invalid in IRI path segments."
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
    (string/replace "}" "%7D")))

(defn lit [s]
  "Escape a string for ntriples literal."
  (let [[escaped (-> s
                   (string/replace "\\" "\\\\")
                   (string/replace "\"" "\\\"")
                   (string/replace "\n" "\\n"))]]
    (string/format "\"{}\"" escaped)))

(defn triple [s p o]
  (push out (string/format "{} {} {} .\n" s p o)))

(def ns "urn:elle")

(defn elle-iri [kind name]
  (iri (string/format "{}:{}:{}" ns kind (encode-name (string name)))))

# ── Form processors ─────────────────────────────────────────────────

(defn process-def [form file]
  "Process (def name value) or (def name). Skip destructuring patterns."
  (when (>= (length form) 2)
    (let [[name-form (get (drop 1 form) 0)]]
      # Only emit for simple symbol bindings, not destructuring
      (when (symbol? name-form)
        (let* [[name (string name-form)]
               [subj (elle-iri "def" name)]]
          (triple subj (iri "http://www.w3.org/1999/02/22-rdf-syntax-ns#type") (iri (string/format "{}:Def" ns)))
          (triple subj (iri (string/format "{}:name" ns)) (lit name))
          (triple subj (iri (string/format "{}:file" ns)) (lit file)))))))

(defn process-defn [form file]
  "Process (defn name [params] docstring? body...)."
  (when (>= (length form) 3)
    (let* [[parts (drop 1 form)]
           [name (string (first parts))]
           [params-form (get parts 1)]
           [subj (elle-iri "fn" name)]
           [param-names (map (fn [p] (string p))
                             (if (array? params-form)
                               (filter (fn [p] (not (= (string p) "&")))
                                       params-form)
                               ()))]]
      (triple subj (iri "http://www.w3.org/1999/02/22-rdf-syntax-ns#type") (iri (string/format "{}:Fn" ns)))
      (triple subj (iri (string/format "{}:name" ns)) (lit name))
      (triple subj (iri (string/format "{}:file" ns)) (lit file))
      (triple subj (iri (string/format "{}:arity" ns)) (lit (string (length param-names))))
      (each p in param-names
        (triple subj (iri (string/format "{}:param" ns)) (lit p)))

      # Check for docstring (string as 3rd element, before body)
      (when (>= (length parts) 4)
        (let [[maybe-doc (get parts 2)]]
          (when (string? maybe-doc)
            (triple subj (iri (string/format "{}:doc" ns)) (lit maybe-doc))))))))

(defn process-defmacro [form file]
  "Process (defmacro name (params) body...)."
  (when (>= (length form) 3)
    (let* [[parts (drop 1 form)]
           [name (string (first parts))]
           [subj (elle-iri "macro" name)]]
      (triple subj (iri "http://www.w3.org/1999/02/22-rdf-syntax-ns#type") (iri (string/format "{}:Macro" ns)))
      (triple subj (iri (string/format "{}:name" ns)) (lit name))
      (triple subj (iri (string/format "{}:file" ns)) (lit file)))))

(defn process-import [form file]
  "Process (def name (import path)) — detect plugin imports."
  (when (>= (length form) 3)
    (let* [[name (string (get (drop 1 form) 0))]
           [val-form (get (drop 1 form) 1)]]
      (when (and (pair? val-form) (= (string (first val-form)) "import"))
        (let* [[path (string (get (drop 1 val-form) 0))]
               [subj (elle-iri "import" name)]]
          (triple subj (iri "http://www.w3.org/1999/02/22-rdf-syntax-ns#type") (iri (string/format "{}:Import" ns)))
          (triple subj (iri (string/format "{}:name" ns)) (lit name))
          (triple subj (iri (string/format "{}:path" ns)) (lit path))
          (triple subj (iri (string/format "{}:file" ns)) (lit file)))))))

# ── Main ─────────────────────────────────────────────────────────────

(each file in files
  (let [[[ok? src] (protect (file/read file))]]
    (when ok?
      (let [[[ok? forms] (protect (read-all src))]]
        (when ok?
          (each form in forms
            (when (pair? form)
              (let [[head (string (first form))]]
                (case head
                  "defn"     (process-defn form file)
                  "defmacro" (process-defmacro form file)
                  "def"      (begin
                               (process-import form file)
                               (process-def form file))
                  nil)))))))))

(println (freeze out))
