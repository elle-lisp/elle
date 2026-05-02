(elle/epoch 9)
## tests/elle/rdf.lisp — verify lib/rdf.lisp triple generation

(def rdf ((import "std/rdf/elle")))

# ── Primitive triples ──────────────────────────────────────────────────

(def prim-triples (rdf:primitives))
(assert (string? prim-triples) "primitives returns a string")
(assert (> (length prim-triples) 1000) "primitives triples are non-trivial")

# Verify a primitive appears (pair is now stdlib, use abs instead)
(assert (string/contains? prim-triples "urn:elle:Primitive")
        "primitives contain Primitive type")
(assert (string/contains? prim-triples "\"abs\"") "primitives contain abs")

# Verify signal metadata is emitted
(assert (string/contains? prim-triples "signal-silent")
        "primitives contain signal-silent predicate")
(assert (string/contains? prim-triples "jit-eligible")
        "primitives contain jit-eligible predicate")

# ── File triples ───────────────────────────────────────────────────────

(def src "(defn add [a b] (+ a b))\n(defn greet [name] (println name))")
(def analysis (compile/analyze src {:file "test.lisp"}))
(def file-triples (rdf:file analysis "test.lisp"))

(assert (string? file-triples) "file returns a string")
(assert (string/contains? file-triples "urn:elle:Fn")
        "file triples contain Fn type")
(assert (string/contains? file-triples "\"add\"")
        "file triples contain add function")
(assert (string/contains? file-triples "\"test.lisp\"")
        "file triples contain file path")

# Verify call graph edges are emitted
(assert (string/contains? file-triples "elle:calls")
        "file triples contain calls edges")

# add calls +, so there should be a calls edge to the + IRI
(assert (string/contains? file-triples "fn:+>")
        "file triples contain calls edge to + primitive")

# ── Encoding ───────────────────────────────────────────────────────────

(assert (= (rdf:encode-name "foo/bar") "foo%2Fbar") "encode-name encodes /")
(assert (= (rdf:encode-name "empty?") "empty%3F") "encode-name encodes ?")
(assert (= (rdf:encode-name "a:b") "a%3Ab") "encode-name encodes :")

# ── Literal escaping ──────────────────────────────────────────────────

(assert (= (rdf:lit "hello") "\"hello\"") "lit wraps in quotes")
(assert (= (rdf:lit "say \"hi\"") "\"say \\\"hi\\\"\"")
        "lit escapes internal quotes")

# ── Re-analysis updates triples ──────────────────────────────────────

(def src2 "(defn add [a b] (+ a b))\n(defn farewell [name] (println name))")
(def analysis2 (compile/analyze src2 {:file "test.lisp"}))
(def file-triples2 (rdf:file analysis2 "test.lisp"))

# farewell should appear, greet should not
(assert (string/contains? file-triples2 "\"farewell\"")
        "re-analyzed triples contain new function farewell")
(assert (not (string/contains? file-triples2 "\"greet\""))
        "re-analyzed triples do not contain removed function greet")

# add should still be present
(assert (string/contains? file-triples2 "\"add\"")
        "re-analyzed triples still contain unchanged function add")

(println "rdf: all tests passed")
