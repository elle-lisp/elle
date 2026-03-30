#!/usr/bin/env elle
## rust-graph.lisp — extract RDF triples from Rust source files
##
## Reads .rs files, parses them with the syn plugin, and emits ntriples
## describing top-level items (fn, struct, enum, trait, impl, const,
## static, type, mod, use).
##
## Usage:
##   elle rust-graph.lisp -- src/main.rs lib.rs ...
##   elle rust-graph.lisp                          # defaults to **/*.rs in CWD

(def glob-plugin (import "glob"))
(def do-glob (get glob-plugin :glob))

(def syn-plugin (import "syn"))
(def parse-file      (get syn-plugin :parse-file))
(def items           (get syn-plugin :items))
(def item-kind       (get syn-plugin :item-kind))
(def item-name       (get syn-plugin :item-name))
(def fn-info         (get syn-plugin :fn-info))
(def fn-calls        (get syn-plugin :fn-calls))
(def primitive-defs  (get syn-plugin :primitive-defs))
(def struct-fields   (get syn-plugin :struct-fields))
(def enum-variants   (get syn-plugin :enum-variants))
(def visibility      (get syn-plugin :visibility))
(def attributes      (get syn-plugin :attributes))
(def to-string       (get syn-plugin :to-string))

(def args (drop 1 (sys/args)))

# map to list — workaround for VM bug (see bug-repro.lisp)
(def files
  (map (fn [f] f)
       (if (empty? args) (do-glob "**/*.rs") args)))

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

(defn lit [s]
  "Escape a string for ntriples literal."
  (let [[escaped (-> s
                   (string/replace "\\" "\\\\")
                   (string/replace "\"" "\\\"")
                   (string/replace "\n" "\\n"))]]
    (string/format "\"{}\"" escaped)))

(defn triple [s p o]
  (push out (string/format "{} {} {} .\n" s p o)))

(def ns "urn:rust")
(def rdf-type (iri "http://www.w3.org/1999/02/22-rdf-syntax-ns#type"))

(defn rust-iri [kind name]
  (iri (string/format "{}:{}:{}" ns kind (encode-name (string name)))))

(defn pred [name]
  (iri (string/format "{}:{}" ns name)))

# ── Item processors ──────────────────────────────────────────────────

(defn emit-visibility [subj item]
  (triple subj (pred "visibility") (lit (string (visibility item)))))

(defn emit-attributes [subj item]
  (each attr in (attributes item)
    (triple subj (pred "attribute") (lit attr))))

(defn process-fn [item file]
  (let* [[info (fn-info item)]
         [name (get info :name)]
         [subj (rust-iri "fn" name)]]
    (triple subj rdf-type (iri (string/format "{}:Fn" ns)))
    (triple subj (pred "name") (lit name))
    (triple subj (pred "file") (lit file))
    (each arg in (get info :args)
      (triple subj (pred "param") (lit (get arg :name)))
      (triple subj (pred "param-type") (lit (string/format "{}:{}" (get arg :name) (get arg :type)))))
    (when (get info :return-type)
      (triple subj (pred "return-type") (lit (get info :return-type))))
    (when (get info :async?)
      (triple subj (pred "async") (lit "true")))
    (when (get info :unsafe?)
      (triple subj (pred "unsafe") (lit "true")))
    (emit-visibility subj item)
    (emit-attributes subj item)
    # Emit call edges from function body.
    (let [[[ok? calls] (protect (fn-calls item))]]
      (when ok?
        (each callee in calls
          (triple subj (pred "calls") (rust-iri "fn" callee)))))))

(defn process-struct [item file]
  (let* [[info (struct-fields item)]
         [name (get info :name)]
         [subj (rust-iri "struct" name)]]
    (triple subj rdf-type (iri (string/format "{}:Struct" ns)))
    (triple subj (pred "name") (lit name))
    (triple subj (pred "file") (lit file))
    (triple subj (pred "kind") (lit (string (get info :kind))))
    (each field in (get info :fields)
      (when (get field :name)
        (triple subj (pred "field") (lit (get field :name)))
        (triple subj (pred "field-type") (lit (string/format "{}:{}" (get field :name) (get field :type))))))
    (emit-visibility subj item)
    (emit-attributes subj item)))

(defn process-enum [item file]
  (let* [[info (enum-variants item)]
         [name (get info :name)]
         [subj (rust-iri "enum" name)]]
    (triple subj rdf-type (iri (string/format "{}:Enum" ns)))
    (triple subj (pred "name") (lit name))
    (triple subj (pred "file") (lit file))
    (each variant in (get info :variants)
      (triple subj (pred "variant") (lit (get variant :name))))
    (emit-visibility subj item)
    (emit-attributes subj item)))

(defn process-trait [item file]
  (let* [[name (item-name item)]
         [subj (rust-iri "trait" name)]]
    (triple subj rdf-type (iri (string/format "{}:Trait" ns)))
    (triple subj (pred "name") (lit name))
    (triple subj (pred "file") (lit file))
    (emit-visibility subj item)
    (emit-attributes subj item)))

(defn process-const [item file]
  (let* [[name (item-name item)]
         [subj (rust-iri "const" name)]]
    (triple subj rdf-type (iri (string/format "{}:Const" ns)))
    (triple subj (pred "name") (lit name))
    (triple subj (pred "file") (lit file))
    (emit-visibility subj item)
    (emit-attributes subj item)))

(defn process-static [item file]
  (let* [[name (item-name item)]
         [subj (rust-iri "static" name)]]
    (triple subj rdf-type (iri (string/format "{}:Static" ns)))
    (triple subj (pred "name") (lit name))
    (triple subj (pred "file") (lit file))
    (emit-visibility subj item)
    (emit-attributes subj item)))

(defn process-type [item file]
  (let* [[name (item-name item)]
         [subj (rust-iri "type" name)]]
    (triple subj rdf-type (iri (string/format "{}:Type" ns)))
    (triple subj (pred "name") (lit name))
    (triple subj (pred "file") (lit file))
    (emit-visibility subj item)
    (emit-attributes subj item)))

(defn process-mod [item file]
  (let* [[name (item-name item)]
         [subj (rust-iri "mod" name)]]
    (triple subj rdf-type (iri (string/format "{}:Mod" ns)))
    (triple subj (pred "name") (lit name))
    (triple subj (pred "file") (lit file))
    (emit-visibility subj item)
    (emit-attributes subj item)))

(defn process-use [item file]
  (let* [[path (to-string item)]
         [subj (iri (string/format "{}:use:{}:{}" ns (encode-name file) (encode-name path)))]]
    (triple subj rdf-type (iri (string/format "{}:Use" ns)))
    (triple subj (pred "path") (lit path))
    (triple subj (pred "file") (lit file))
    (emit-visibility subj item)))

# ── Primitive mapping ────────────────────────────────────────────────
# Extract Elle name → Rust function links from PRIMITIVES tables.
# Each PrimitiveDef { name: "elle/name", func: rust_fn_name, ... }
# produces: <urn:elle:fn:elle%2Fname> <urn:elle:implemented-by> <urn:rust:fn:rust_fn_name>

(def elle-ns "urn:elle")

(defn elle-iri [name]
  (iri (string/format "{}:fn:{}" elle-ns (encode-name (string name)))))

(defn elle-pred [name]
  (iri (string/format "{}:{}" elle-ns name)))

(defn extract-primitive-mappings [tree file]
  "Walk all items looking for PRIMITIVES consts and extract name→func mappings."
  (each item in (items tree)
    (when (= (item-kind item) :const)
      (var name (item-name item))
      (when (= name "PRIMITIVES")
        (var defs (primitive-defs item))
        (each def in defs
          (var elle-name (get def :name))
          (var rust-fn   (get def :func))
          (triple (elle-iri elle-name)
                  (elle-pred "implemented-by")
                  (rust-iri "fn" rust-fn))
          (triple (rust-iri "fn" rust-fn)
                  (pred "implements")
                  (elle-iri elle-name)))))))

# ── Main ─────────────────────────────────────────────────────────────

(each file in files
  (let [[[ok? src] (protect (file/read file))]]
    (when ok?
      (let [[[ok? tree] (protect (parse-file src))]]
        (if (not ok?)
          (eprintln "warning: parse error in " file ": " (get tree :message))
          (begin
            (each item in (items tree)
              (let [[kind (item-kind item)]]
                (case kind
                  :fn      (process-fn item file)
                  :struct  (process-struct item file)
                  :enum    (process-enum item file)
                  :trait   (process-trait item file)
                  :const   (process-const item file)
                  :static  (process-static item file)
                  :type    (process-type item file)
                  :mod     (process-mod item file)
                  :use     (process-use item file)
                  nil)))
            # Extract primitive name→func mappings from PRIMITIVES tables.
            (extract-primitive-mappings tree file)))))))

(print (freeze out))
