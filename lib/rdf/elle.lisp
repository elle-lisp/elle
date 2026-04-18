(elle/epoch 7)
## lib/rdf/elle.lisp — RDF triple generation for Elle source analysis
##
## Canonical representation of Elle semantic data as N-Triples.
## Used by both the batch extractor (tools/semantic-graph.lisp) and the
## MCP server (tools/mcp-server.lisp) to ensure a single, consistent
## schema for the knowledge graph.
##
## Usage:
##   (def rdf ((import "std/rdf/elle")))
##   (def triples (rdf:primitives))       # N-Triples string for all Rust prims
##   (def triples (rdf:file analysis "path.lisp"))  # for an analyzed Elle file

(fn []

(def portrait-lib ((import "std/portrait")))

# ── Namespace ──────────────────────────────────────────────────────────

(def ns "urn:elle")
(def rdf-type "<http://www.w3.org/1999/02/22-rdf-syntax-ns#type>")

# ── Encoding helpers ───────────────────────────────────────────────────

(defn encode-name [name]
  "URL-encode characters that are invalid in IRI local names."
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
    (string/replace "/" "%2F")
    (string/replace ":" "%3A")))

(defn iri [s]
  (string/format "<{}>" s))

(defn elle-iri [kind name]
  (iri (string/format "{}:{}:{}" ns kind (encode-name (string name)))))

(defn pred [p]
  (iri (string/format "{}:{}" ns p)))

(defn lit [s]
  "Escape a string for use as an N-Triples literal."
  (var escaped (-> (string s)
                 (string/replace "\\" "\\\\")
                 (string/replace "\"" "\\\"")
                 (string/replace "\n" "\\n")))
  (string/format "\"{}\"" escaped))

# ── Triple buffer ─────────────────────────────────────────────────────

(defn make-buffer []
  "Create a fresh mutable string buffer for accumulating triples."
  @"")

(defn triple [buf s p o]
  "Append a single N-Triple to a buffer."
  (push buf (string/format "{} {} {} .\n" s p o)))

# ── Signal triples ─────────────────────────────────────────────────────

(defn emit-signal [buf subj sig]
  "Emit signal-related triples for a subject."
  (triple buf subj (pred "signal-silent") (lit (string (get sig :silent))))
  (triple buf subj (pred "signal-yields") (lit (string (get sig :yields))))
  (triple buf subj (pred "signal-io") (lit (string (get sig :io))))
  (triple buf subj (pred "jit-eligible") (lit (string (get sig :jit-eligible))))
  (each bit in (get sig :bits)
    (triple buf subj (pred "signal-bit") (lit (string bit)))))

# ── Primitive triples ──────────────────────────────────────────────────

(defn emit-primitive [buf prim]
  "Emit triples for a single Rust primitive."
  (var name (get prim :name))
  (var subj (elle-iri "fn" name))

  (triple buf subj rdf-type (iri (string/format "{}:Primitive" ns)))
  (triple buf subj (pred "name") (lit name))
  (triple buf subj (pred "category") (lit (get prim :category)))
  (triple buf subj (pred "arity") (lit (get prim :arity)))

  (when (not (= (get prim :doc) ""))
    (triple buf subj (pred "doc") (lit (get prim :doc))))

  (emit-signal buf subj (get prim :signal))

  (each p in (get prim :params)
    (triple buf subj (pred "param") (lit p)))

  (each a in (get prim :aliases)
    (triple buf subj (pred "alias") (lit a))))

(defn primitives []
  "Generate N-Triples for all Rust-defined primitives."
  (var buf (make-buffer))
  (each prim in (compile/primitives)
    (emit-primitive buf prim))
  (freeze buf))

# ── Elle function triples ──────────────────────────────────────────────

(defn emit-function [buf analysis path sym]
  "Emit triples for an Elle-defined function."
  (var name (get sym :name))
  (var subj (elle-iri "fn" name))

  (triple buf subj rdf-type (iri (string/format "{}:Fn" ns)))
  (triple buf subj (pred "name") (lit name))
  (triple buf subj (pred "file") (lit path))

  (when (get sym :arity)
    (triple buf subj (pred "arity") (lit (string (get sym :arity)))))
  (when (get sym :doc)
    (triple buf subj (pred "doc") (lit (get sym :doc))))
  (when (get sym :line)
    (triple buf subj (pred "line") (lit (string (get sym :line)))))

  # Signal
  (var sig nil)
  (let [[ok? val] (protect (compile/signal analysis (keyword name)))]
    (when ok? (assign sig val)))

  (when sig
    (emit-signal buf subj sig)

    (each idx in (get sig :propagates)
      (triple buf subj (pred "signal-propagates") (lit (string idx))))

    # Captures
    (var caps nil)
    (let [[ok? val] (protect (compile/captures analysis (keyword name)))]
      (when ok? (assign caps val)))

    (when caps
      (each cap in caps
        (triple buf subj (pred "capture") (lit (get cap :name)))
        (triple buf subj (pred "capture-kind") (lit (string (get cap :kind)))))

      # Composition
      (var comp (portrait-lib:composition sig caps))
      (triple buf subj (pred "stateless") (lit (string (get comp :stateless))))
      (triple buf subj (pred "retry-safe") (lit (string (get comp :retry-safe))))
      (triple buf subj (pred "parallelizable") (lit (string (get comp :parallelizable))))
      (triple buf subj (pred "memoizable") (lit (string (get comp :memoizable))))
      (triple buf subj (pred "timeout-safe") (lit (string (get comp :timeout-safe)))))))

# ── File triples ───────────────────────────────────────────────────────

(defn file [analysis path]
  "Generate N-Triples for an analyzed Elle file."
  (var buf (make-buffer))
  (var syms (compile/symbols analysis))
  (var graph (compile/call-graph analysis))

  (each sym in syms
    (var kind (get sym :kind))
    (var name (get sym :name))

    (when (= kind :function)
      (emit-function buf analysis path sym))

    (when (= kind :variable)
      (var subj (elle-iri "def" name))
      (triple buf subj rdf-type (iri (string/format "{}:Def" ns)))
      (triple buf subj (pred "name") (lit name))
      (triple buf subj (pred "file") (lit path)))

    (when (= kind :macro)
      (var subj (elle-iri "macro" name))
      (triple buf subj rdf-type (iri (string/format "{}:Macro" ns)))
      (triple buf subj (pred "name") (lit name))
      (triple buf subj (pred "file") (lit path))))

  # Call graph edges
  (each node in (get graph :nodes)
    (var caller-iri (elle-iri "fn" (get node :name)))
    (each callee-name in (get node :callees)
      (triple buf caller-iri (pred "calls") (elle-iri "fn" callee-name))))

  (freeze buf))

# ── Export ──────────────────────────────────────────────────────────────

{:primitives   primitives
 :file         file
 :make-buffer  make-buffer
 :triple       triple
 :elle-iri     elle-iri
 :pred         pred
 :lit          lit
 :encode-name  encode-name
 :emit-signal  emit-signal})  # end closure
