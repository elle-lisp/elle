(elle/epoch 9)
## lib/rdf/rust.lisp — RDF triple generation for Rust source
##
## Parses .rs files with the syn plugin and emits N-Triples for functions,
## structs, enums, traits, and call edges. Also extracts PRIMITIVES tables
## to link Elle primitives to their Rust implementations.
##
## Usage:
##   (def syn (import "plugin/syn"))
##   (def rust-rdf ((import "std/rdf/rust") syn))
##   (def triples (rust-rdf:file "src/main.rs"))
##   (def links   (rust-rdf:primitive-links "src/primitives/io.rs"))

(fn [syn]

  # ── Namespace ──────────────────────────────────────────────────────────

  (def ns "urn:rust")
  (def elle-ns "urn:elle")
  (def rdf-type "<http://www.w3.org/1999/02/22-rdf-syntax-ns#type>")

  # ── Encoding helpers ───────────────────────────────────────────────────

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
  (defn iri [s]
    (string/format "<{}>" s))
  (defn rust-iri [kind name]
    (iri (string/format "{}:{}:{}" ns kind (encode-name (string name)))))
  (defn elle-iri [name]
    (iri (string/format "{}:fn:{}" elle-ns (encode-name (string name)))))
  (defn pred [p]
    (iri (string/format "{}:{}" ns p)))
  (defn elle-pred [p]
    (iri (string/format "{}:{}" elle-ns p)))
  (defn lit [s]
    "Escape a string for N-Triples literal."
    (def @escaped
      (-> (string s)
          (string/replace "\\" "\\\\")
          (string/replace "\"" "\\\"")
          (string/replace "\n" "\\n")))
    (string/format "\"{}\"" escaped))

  # ── Triple buffer ─────────────────────────────────────────────────────

  (defn make-buffer []
    (thaw ""))
  (defn triple [buf s p o]
    (push buf (string/format "{} {} {} .\n" s p o)))

  # ── Item emitters ─────────────────────────────────────────────────────

  (defn emit-visibility [buf subj item]
    (triple buf subj (pred "visibility") (lit (string (syn:visibility item)))))
  (defn emit-attributes [buf subj item]
    (each attr in (syn:attributes item)
      (triple buf subj (pred "attribute") (lit attr))))
  (defn emit-fn [buf item file]
    (let* [info (syn:fn-info item)
           name (get info :name)
           subj (rust-iri "fn" name)]
      (triple buf subj rdf-type (iri (string/format "{}:Fn" ns)))
      (triple buf subj (pred "name") (lit name))
      (triple buf subj (pred "file") (lit file))
      (when (get info :line)
        (triple buf subj (pred "line") (lit (string (get info :line)))))
      (each arg in (get info :args)
        (triple buf subj (pred "param") (lit (get arg :name)))
        (triple buf subj (pred "param-type")
          (lit (string/format "{}:{}" (get arg :name) (get arg :type)))))
      (when (get info :return-type)
        (triple buf subj (pred "return-type") (lit (get info :return-type))))
      (when (get info :async?) (triple buf subj (pred "async") (lit "true")))
      (when (get info :unsafe?) (triple buf subj (pred "unsafe") (lit "true")))
      (emit-visibility buf subj item)
      (emit-attributes buf subj item)
      (let [[ok? calls] (protect (syn:fn-calls item))]
        (when ok?
          (each callee in calls
            (triple buf subj (pred "calls") (rust-iri "fn" callee)))))))
  (defn emit-struct [buf item file]
    (let* [info (syn:struct-fields item)
           name (get info :name)
           subj (rust-iri "struct" name)
           line (syn:item-line item)]
      (triple buf subj rdf-type (iri (string/format "{}:Struct" ns)))
      (triple buf subj (pred "name") (lit name))
      (triple buf subj (pred "file") (lit file))
      (when line
        (triple buf subj (pred "line") (lit (string line))))
      (triple buf subj (pred "kind") (lit (string (get info :kind))))
      (each field in (get info :fields)
        (when (get field :name)
          (triple buf subj (pred "field") (lit (get field :name)))
          (triple buf subj (pred "field-type")
            (lit (string/format "{}:{}" (get field :name) (get field :type))))))
      (emit-visibility buf subj item)
      (emit-attributes buf subj item)))
  (defn emit-enum [buf item file]
    (let* [info (syn:enum-variants item)
           name (get info :name)
           subj (rust-iri "enum" name)
           line (syn:item-line item)]
      (triple buf subj rdf-type (iri (string/format "{}:Enum" ns)))
      (triple buf subj (pred "name") (lit name))
      (triple buf subj (pred "file") (lit file))
      (when line
        (triple buf subj (pred "line") (lit (string line))))
      (each variant in (get info :variants)
        (triple buf subj (pred "variant") (lit (get variant :name))))
      (emit-visibility buf subj item)
      (emit-attributes buf subj item)))
  (defn emit-named [buf kind-str item file]
    (let* [name (syn:item-name item)
           subj (rust-iri kind-str name)
           line (syn:item-line item)]
      (triple buf subj rdf-type (iri (string/format "{}:{}" ns kind-str)))
      (triple buf subj (pred "name") (lit name))
      (triple buf subj (pred "file") (lit file))
      (when line
        (triple buf subj (pred "line") (lit (string line))))
      (emit-visibility buf subj item)
      (emit-attributes buf subj item)))
  (defn emit-use [buf item file]
    (let* [path (syn:to-string item)
           subj (iri (string/format "{}:use:{}:{}" ns (encode-name file)
                       (encode-name path)))]
      (triple buf subj rdf-type (iri (string/format "{}:Use" ns)))
      (triple buf subj (pred "path") (lit path))
      (triple buf subj (pred "file") (lit file))
      (emit-visibility buf subj item)))

  # ── File extraction ───────────────────────────────────────────────────

  (defn extract-file [file]
    "Parse a Rust file and return N-Triples for all items."
    (def @buf (make-buffer))
    (def @src (file/read file))
    (def @tree (syn:parse-file src))
    (each item in (syn:items tree)
      (case (syn:item-kind item)
        :fn (emit-fn buf item file)
        :struct (emit-struct buf item file)
        :enum (emit-enum buf item file)
        :trait (emit-named buf "Trait" item file)
        :const (emit-named buf "Const" item file)
        :static (emit-named buf "Static" item file)
        :type (emit-named buf "Type" item file)
        :mod (emit-named buf "Mod" item file)
        :use (emit-use buf item file)
        nil))
    (freeze buf))

  # ── Primitive cross-links ─────────────────────────────────────────────

  (defn extract-primitive-links [file]
    "Parse a Rust file and return N-Triples linking Elle primitives to Rust fns."
    (def @buf (make-buffer))
    (def @src (file/read file))
    (def @tree (syn:parse-file src))
    (each item in (syn:items tree)
      (when (and (= (syn:item-kind item) :const)
          (= (syn:item-name item) "PRIMITIVES"))
        (let [[ok? defs] (protect (syn:primitive-defs item))]
          (when ok?
            (each def in defs
              (def @elle-name (get def :name))
              (def @rust-fn (get def :func))
              (triple buf (elle-iri elle-name) (elle-pred "implemented-by")
                (rust-iri "fn" rust-fn))
              (triple buf (rust-iri "fn" rust-fn) (pred "implements")
                (elle-iri elle-name)))))))
    (freeze buf))

  # ── Export ────────────────────────────────────────────────────────────

  {:file extract-file
   :primitive-links extract-primitive-links
   :make-buffer make-buffer
   :triple triple
   :rust-iri rust-iri
   :pred pred
   :lit lit
   :encode-name encode-name})
# end closure
