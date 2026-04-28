(elle/epoch 9)

## Oxigraph plugin integration tests
## Tests the oxigraph plugin (.so loaded via import-file)

## Try to load the oxigraph plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_oxigraph.so")))
(when (not ok?)
  (print "SKIP: oxigraph plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def store-new (get plugin :store-new))
(def store-open (get plugin :store-open))
(def iri (get plugin :iri))
(def literal (get plugin :literal))
(def blank-node (get plugin :blank-node))

## ── Scenario 1: Store creation ─────────────────────────────────────

(assert (not (nil? (store-new))) "store-new returns non-nil")

(def tmp-path "/tmp/elle-oxigraph-test-store")

(assert (not (nil? (store-open tmp-path)))
  "store-open with temp path returns non-nil")

## ── Scenario 2: Term constructors ──────────────────────────────────

## IRI: 2-element array [:iri "http://..."]
(def alice-iri (iri "http://example.org/alice"))

(assert (= (get alice-iri 0) :iri) "iri: first element is :iri keyword")

(assert (= (get alice-iri 1) "http://example.org/alice")
  "iri: second element is the IRI string")

(assert (= (length alice-iri) 2) "iri: array has length 2")

## Plain literal: 2-element array [:literal "..."]
(def hello-lit (literal "hello"))

(assert (= (get hello-lit 0) :literal)
  "literal: first element is :literal keyword")

(assert (= (get hello-lit 1) "hello")
  "literal: second element is the value string")

(assert (= (length hello-lit) 2) "literal: plain array has length 2")

## Language-tagged literal: 4-element array [:literal "..." :lang "en"]
(def lang-lit (literal "hello" :lang "en"))

(assert (= (get lang-lit 0) :literal)
  "literal with lang: first element is :literal")

(assert (= (get lang-lit 1) "hello")
  "literal with lang: second element is value")

(assert (= (get lang-lit 2) :lang) "literal with lang: third element is :lang")

(assert (= (get lang-lit 3) "en")
  "literal with lang: fourth element is language tag")

(assert (= (length lang-lit) 4) "literal with lang: array has length 4")

## Datatype literal: 4-element array [:literal "..." :datatype "http://..."]
(def xsd-int "http://www.w3.org/2001/XMLSchema#integer")
(def typed-lit (literal "42" :datatype xsd-int))

(assert (= (get typed-lit 0) :literal)
  "literal with datatype: first element is :literal")

(assert (= (get typed-lit 1) "42")
  "literal with datatype: second element is value")

(assert (= (get typed-lit 2) :datatype)
  "literal with datatype: third element is :datatype")

(assert (= (get typed-lit 3) xsd-int)
  "literal with datatype: fourth element is datatype IRI")

(assert (= (length typed-lit) 4) "literal with datatype: array has length 4")

## Blank node auto-generated: 2-element array [:bnode "..."] with non-empty id
(def auto-bnode (blank-node))

(assert (= (get auto-bnode 0) :bnode) "blank-node auto: first element is :bnode")

(assert (> (length (get auto-bnode 1)) 0)
  "blank-node auto: id is non-empty string")

(assert (= (length auto-bnode) 2) "blank-node auto: array has length 2")

## Blank node explicit id: [:bnode "b1"]
(def named-bnode (blank-node "b1"))

(assert (= (get named-bnode 0) :bnode)
  "blank-node explicit: first element is :bnode")

(assert (= (get named-bnode 1) "b1")
  "blank-node explicit: second element is the given id")

## Malformed IRI signals oxigraph-error
(let [[ok? err] (protect ((fn () (iri "not a valid IRI"))))]
  (assert (not ok?) "iri with malformed IRI signals oxigraph-error")
  (assert (= (get err :error) :oxigraph-error)
    "iri with malformed IRI signals oxigraph-error"))

## ── Scenario 3: Quad CRUD ──────────────────────────────────────────

(def insert (get plugin :insert))
(def remove (get plugin :remove))
(def contains (get plugin :contains))
(def quads (get plugin :quads))

(def ex-s (iri "http://example.org/alice"))
(def ex-p (iri "http://xmlns.com/foaf/0.1/name"))
(def ex-o (literal "Alice"))
(def ex-g (iri "http://example.org/graph1"))

(def quad-default [ex-s ex-p ex-o nil])
(def quad-named [ex-s ex-p ex-o ex-g])

## quads on empty store returns empty array
(def fresh-store (store-new))
(assert (= (length (quads fresh-store)) 0)
  "quads on empty store returns array of length 0")

## insert with nil graph, contains returns true
(def store1 (store-new))
(insert store1 quad-default)
(assert (contains store1 quad-default)
  "contains returns true after insert with nil graph")

## insert with named graph, contains returns true
(def store2 (store-new))
(insert store2 quad-named)
(assert (contains store2 quad-named)
  "contains returns true after insert with named graph")

## contains on absent quad returns false
(def store3 (store-new))
(assert (not (contains store3 quad-default))
  "contains returns false for quad not in store")

## insert two quads, quads returns array of length 2
(def store4 (store-new))
(insert store4 quad-default)
(insert store4 quad-named)
(assert (= (length (quads store4)) 2)
  "quads returns array of length 2 after two inserts")

## remove a quad, contains returns false, quads length decreases
(def store5 (store-new))
(insert store5 quad-default)
(insert store5 quad-named)
(remove store5 quad-default)
(assert (not (contains store5 quad-default))
  "contains returns false after remove")
(assert (= (length (quads store5)) 1) "quads length decreases by 1 after remove")

## remove non-existent quad is a no-op (no error)
(def store6 (store-new))
(remove store6 quad-default)
(assert (= (length (quads store6)) 0)
  "remove of non-existent quad leaves store unchanged")

## structural verification: quad array elements are term arrays
(def store7 (store-new))
(insert store7 quad-default)
(def result-quads (quads store7))
(def q (get result-quads 0))

## subject: [:iri "http://example.org/alice"]
(assert (= (get q 0) [:iri "http://example.org/alice"])
  "quad element 0 (subject) is [:iri ...] array")

## predicate: [:iri "http://xmlns.com/foaf/0.1/name"]
(assert (= (get q 1) [:iri "http://xmlns.com/foaf/0.1/name"])
  "quad element 1 (predicate) is [:iri ...] array")

## object: [:literal "Alice"]
(assert (= (get q 2) [:literal "Alice"])
  "quad element 2 (object) is [:literal ...] array")

## graph-name: nil (default graph)
(assert (= (get q 3) nil) "quad element 3 (graph-name) is nil for default graph")

## ── Scenario 4: SPARQL SELECT ──────────────────────────────────────

(def query (get plugin :query))
(def update (get plugin :update))

## Set up a store with a few quads for SPARQL tests
(def sparql-store (store-new))
(def alice (iri "http://example.org/alice"))
(def bob (iri "http://example.org/bob"))
(def name (iri "http://xmlns.com/foaf/0.1/name"))
(def knows (iri "http://xmlns.com/foaf/0.1/knows"))
(insert sparql-store [alice name (literal "Alice") nil])
(insert sparql-store [bob name (literal "Bob") nil])
(insert sparql-store [alice knows bob nil])

## SELECT returns an array of binding structs
(def select-results
  (query sparql-store
    "SELECT ?s ?o WHERE { ?s <http://xmlns.com/foaf/0.1/name> ?o }"))

(assert (> (length select-results) 0) "SELECT returns at least one row")

## Each row is a struct with keyword keys; check the first row has :s and :o
(def first-row (get select-results 0))

(assert (not (nil? (get first-row :s))) "SELECT row has :s binding")

(assert (not (nil? (get first-row :o))) "SELECT row has :o binding")

## Binding values are term arrays: subject should be an [:iri ...] or [:bnode ...]
(assert (= (get (get first-row :s) 0) :iri) "SELECT binding :s is an IRI term")

## Binding values for literal: object should be [:literal ...]
(def row-with-alice
  (letrec [find-alice (fn (i)
                        (if (>= i (length select-results))
                          nil
                          (let [row (get select-results i)]
                            (if (= (get (get row :s) 1)
                                "http://example.org/alice")
                              row
                              (find-alice (+ i 1))))))]
    (find-alice 0)))

(assert (not (nil? row-with-alice)) "SELECT result contains row for alice")

(assert (= (get (get row-with-alice :o) 0) :literal)
  "SELECT binding :o for alice is a literal term")

(assert (= (get (get row-with-alice :o) 1) "Alice")
  "SELECT binding :o for alice has value 'Alice'")

## Unbound variables are omitted (not nil): use OPTIONAL to get an unbound var
(def optional-results
  (query sparql-store
    (string "SELECT ?s ?missing WHERE { ?s "
      "<http://xmlns.com/foaf/0.1/name> ?o " ". OPTIONAL { ?s "
      "<http://example.org/noSuchProp> " "?missing } }")))

(assert (> (length optional-results) 0) "SELECT with OPTIONAL returns rows")

## The unbound :missing key should be absent (nil? returns true when key not present)
(def first-optional-row (get optional-results 0))
(assert (nil? (get first-optional-row :missing))
  "Unbound variable is absent from binding struct")

## ── Scenario 5: SPARQL ASK ─────────────────────────────────────────

## ASK returns true when matching quads exist
(assert (query sparql-store
    "ASK { <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> \"Alice\" }")
  "ASK returns true when triple exists")

## ASK returns false when no match
(assert (not (query sparql-store
               "ASK { <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> \"Nobody\" }"))
  "ASK returns false when triple does not exist")

## ── Scenario 6: SPARQL CONSTRUCT ───────────────────────────────────

## CONSTRUCT returns array of quad arrays (with nil graph-name)
(def construct-results
  (query sparql-store
    "CONSTRUCT { ?s <http://xmlns.com/foaf/0.1/name> ?o } WHERE { ?s <http://xmlns.com/foaf/0.1/name> ?o }"))

(assert (> (length construct-results) 0) "CONSTRUCT returns at least one quad")

## Each element is a 4-element array
(def first-construct-quad (get construct-results 0))
(assert (= (length first-construct-quad) 4)
  "CONSTRUCT result element is a 4-element array")

## Subject is an IRI term
(assert (= (get (get first-construct-quad 0) 0) :iri)
  "CONSTRUCT quad subject is an IRI")

## Predicate is an IRI term
(assert (= (get (get first-construct-quad 1) 0) :iri)
  "CONSTRUCT quad predicate is an IRI")

## Graph-name is nil (CONSTRUCT produces triples, default graph)
(assert (= (get first-construct-quad 3) nil) "CONSTRUCT quad graph-name is nil")

## ── Scenario 7: SPARQL UPDATE ──────────────────────────────────────

(def update-store (store-new))

## INSERT DATA inserts quads, verify with contains
(update update-store
  "INSERT DATA { <http://example.org/x> <http://example.org/p> \"hello\" }")

(assert (contains update-store
    [(iri "http://example.org/x") (iri "http://example.org/p") (literal "hello")
     nil]) "INSERT DATA via update inserts triple")

## DELETE DATA removes quads, verify removal
(update update-store
  "DELETE DATA { <http://example.org/x> <http://example.org/p> \"hello\" }")

(assert (not (contains update-store
               [(iri "http://example.org/x") (iri "http://example.org/p")
                (literal "hello") nil])) "DELETE DATA via update removes triple")

## Malformed SPARQL UPDATE signals sparql-error
(let [[ok? err] (protect ((fn () (update update-store "THIS IS NOT SPARQL"))))]
  (assert (not ok?) "malformed SPARQL UPDATE signals sparql-error")
  (assert (= (get err :error) :sparql-error)
    "malformed SPARQL UPDATE signals sparql-error"))

## ── Scenario 8: Load/dump roundtrip ───────────────────────────────

(def load (get plugin :load))
(def dump (get plugin :dump))

## Load N-Triples data into a fresh store, dump as N-Triples, verify content
(def nt-store (store-new))
(def nt-data "<http://example.org/s> <http://example.org/p> \"hello\" .\n")
(load nt-store nt-data :ntriples)
(def nt-dumped (dump nt-store :ntriples))

(assert (string/contains? nt-dumped "<http://example.org/s>")
  "dump :ntriples output contains subject IRI")

(assert (string/contains? nt-dumped "\"hello\"")
  "dump :ntriples output contains literal value")

## Load N-Triples data, verify quads are present via query
(def nt-store2 (store-new))
(load nt-store2
  "<http://example.org/alice> <http://xmlns.com/foaf/0.1/name> \"Alice\" .\n"
  :ntriples)

(assert (query nt-store2
    "ASK { <http://example.org/alice> <http://xmlns.com/foaf/0.1/name> \"Alice\" }")
  "loaded N-Triples triple is queryable via ASK")

## Load N-Quads data with a named graph, verify graph-name comes back from quads
(def nq-store (store-new))
(def nq-data
  "<http://example.org/s> <http://example.org/p> <http://example.org/o> <http://example.org/graph1> .\n")
(load nq-store nq-data :nquads)
(def nq-quads (quads nq-store))

(assert (= (length nq-quads) 1) "loaded N-Quads store has 1 quad")

(def nq-quad (get nq-quads 0))
(assert (= (get nq-quad 3) [:iri "http://example.org/graph1"])
  "loaded N-Quads quad has correct graph-name")

## Unknown format keyword signals type-error
(def fmt-store (store-new))
(let [[ok? err] (protect ((fn ()
                            (load fmt-store
                              "<http://example.org/s> <http://example.org/p> \"x\" .\n"
                              :unknown-format))))]
  (assert (not ok?) "unknown format keyword signals type-error on load")
  (assert (= (get err :error) :type-error)
    "unknown format keyword signals type-error on load"))

(let [[ok? err] (protect ((fn () (dump fmt-store :unknown-format))))]
  (assert (not ok?) "unknown format keyword signals type-error on dump")
  (assert (= (get err :error) :type-error)
    "unknown format keyword signals type-error on dump"))

## ── Scenario 9: Error cases ────────────────────────────────────────

## Wrong type for store argument (pass a string instead)
(let [[ok? err] (protect ((fn () (quads "not-a-store"))))]
  (assert (not ok?) "wrong type for store argument signals type-error")
  (assert (= (get err :error) :type-error)
    "wrong type for store argument signals type-error"))

## Wrong type for quad argument (pass a string instead of array)
(def err-store (store-new))
(let [[ok? err] (protect ((fn () (insert err-store "not-a-quad"))))]
  (assert (not ok?) "wrong type for quad argument signals type-error")
  (assert (= (get err :error) :type-error)
    "wrong type for quad argument signals type-error"))

## Quad array with wrong length (3 elements)
(let [[ok? err] (protect ((fn ()
                            (insert err-store
                              [(iri "http://example.org/s")
                               (iri "http://example.org/p") (literal "o")]))))]
  (assert (not ok?) "quad array with wrong length signals type-error")
  (assert (= (get err :error) :type-error)
    "quad array with wrong length signals type-error"))

## Malformed SPARQL query signals sparql-error
(let [[ok? err] (protect ((fn () (query err-store "THIS IS NOT SPARQL"))))]
  (assert (not ok?) "malformed SPARQL query signals sparql-error")
  (assert (= (get err :error) :sparql-error)
    "malformed SPARQL query signals sparql-error"))

## Malformed IRI in quad signals oxigraph-error
(let [[ok? err] (protect ((fn ()
                            (insert err-store
                              [[:iri "not-an-iri"] (iri "http://example.org/p")
                               (literal "o") nil]))))]
  (assert (not ok?) "malformed IRI in quad signals oxigraph-error")
  (assert (= (get err :error) :oxigraph-error)
    "malformed IRI in quad signals oxigraph-error"))
