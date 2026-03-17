(def {:assert-eq assert-eq
      :assert-true assert-true
      :assert-false assert-false
      :assert-not-nil assert-not-nil
      :assert-err-kind assert-err-kind} ((import-file "tests/elle/assert.lisp")))

## Oxigraph plugin integration tests
## Tests the oxigraph plugin (.so loaded via import-file)

## Try to load the oxigraph plugin. If it fails, exit cleanly.
(def [ok? plugin] (protect (import-file "target/release/libelle_oxigraph.so")))
(when (not ok?)
  (display "SKIP: oxigraph plugin not built\n")
  (exit 0))

## Extract plugin functions from the returned struct
(def store-new    (get plugin :store-new))
(def store-open   (get plugin :store-open))
(def iri          (get plugin :iri))
(def literal      (get plugin :literal))
(def blank-node   (get plugin :blank-node))

## ── Scenario 1: Store creation ─────────────────────────────────────

(assert-not-nil
  (store-new)
  "store-new returns non-nil")

(def tmp-path "/tmp/elle-oxigraph-test-store")

(assert-not-nil
  (store-open tmp-path)
  "store-open with temp path returns non-nil")

## ── Scenario 2: Term constructors ──────────────────────────────────

## IRI: 2-element array [:iri "http://..."]
(def alice-iri (iri "http://example.org/alice"))

(assert-eq
  (get alice-iri 0)
  :iri
  "iri: first element is :iri keyword")

(assert-eq
  (get alice-iri 1)
  "http://example.org/alice"
  "iri: second element is the IRI string")

(assert-eq
  (length alice-iri)
  2
  "iri: array has length 2")

## Plain literal: 2-element array [:literal "..."]
(def hello-lit (literal "hello"))

(assert-eq
  (get hello-lit 0)
  :literal
  "literal: first element is :literal keyword")

(assert-eq
  (get hello-lit 1)
  "hello"
  "literal: second element is the value string")

(assert-eq
  (length hello-lit)
  2
  "literal: plain array has length 2")

## Language-tagged literal: 4-element array [:literal "..." :lang "en"]
(def lang-lit (literal "hello" :lang "en"))

(assert-eq
  (get lang-lit 0)
  :literal
  "literal with lang: first element is :literal")

(assert-eq
  (get lang-lit 1)
  "hello"
  "literal with lang: second element is value")

(assert-eq
  (get lang-lit 2)
  :lang
  "literal with lang: third element is :lang")

(assert-eq
  (get lang-lit 3)
  "en"
  "literal with lang: fourth element is language tag")

(assert-eq
  (length lang-lit)
  4
  "literal with lang: array has length 4")

## Datatype literal: 4-element array [:literal "..." :datatype "http://..."]
(def xsd-int "http://www.w3.org/2001/XMLSchema#integer")
(def typed-lit (literal "42" :datatype xsd-int))

(assert-eq
  (get typed-lit 0)
  :literal
  "literal with datatype: first element is :literal")

(assert-eq
  (get typed-lit 1)
  "42"
  "literal with datatype: second element is value")

(assert-eq
  (get typed-lit 2)
  :datatype
  "literal with datatype: third element is :datatype")

(assert-eq
  (get typed-lit 3)
  xsd-int
  "literal with datatype: fourth element is datatype IRI")

(assert-eq
  (length typed-lit)
  4
  "literal with datatype: array has length 4")

## Blank node auto-generated: 2-element array [:bnode "..."] with non-empty id
(def auto-bnode (blank-node))

(assert-eq
  (get auto-bnode 0)
  :bnode
  "blank-node auto: first element is :bnode")

(assert-true
  (> (length (get auto-bnode 1)) 0)
  "blank-node auto: id is non-empty string")

(assert-eq
  (length auto-bnode)
  2
  "blank-node auto: array has length 2")

## Blank node explicit id: [:bnode "b1"]
(def named-bnode (blank-node "b1"))

(assert-eq
  (get named-bnode 0)
  :bnode
  "blank-node explicit: first element is :bnode")

(assert-eq
  (get named-bnode 1)
  "b1"
  "blank-node explicit: second element is the given id")

## Malformed IRI signals oxigraph-error
(assert-err-kind
  (fn () (iri "not a valid IRI"))
  :oxigraph-error
  "iri with malformed IRI signals oxigraph-error")

## ── Scenario 3: Quad CRUD ──────────────────────────────────────────

(def insert   (get plugin :insert))
(def remove   (get plugin :remove))
(def contains (get plugin :contains))
(def quads    (get plugin :quads))

(def ex-s  (iri "http://example.org/alice"))
(def ex-p  (iri "http://xmlns.com/foaf/0.1/name"))
(def ex-o  (literal "Alice"))
(def ex-g  (iri "http://example.org/graph1"))

(def quad-default [ex-s ex-p ex-o nil])
(def quad-named   [ex-s ex-p ex-o ex-g])

## quads on empty store returns empty array
(def fresh-store (store-new))
(assert-eq
  (length (quads fresh-store))
  0
  "quads on empty store returns array of length 0")

## insert with nil graph, contains returns true
(def store1 (store-new))
(insert store1 quad-default)
(assert-true
  (contains store1 quad-default)
  "contains returns true after insert with nil graph")

## insert with named graph, contains returns true
(def store2 (store-new))
(insert store2 quad-named)
(assert-true
  (contains store2 quad-named)
  "contains returns true after insert with named graph")

## contains on absent quad returns false
(def store3 (store-new))
(assert-false
  (contains store3 quad-default)
  "contains returns false for quad not in store")

## insert two quads, quads returns array of length 2
(def store4 (store-new))
(insert store4 quad-default)
(insert store4 quad-named)
(assert-eq
  (length (quads store4))
  2
  "quads returns array of length 2 after two inserts")

## remove a quad, contains returns false, quads length decreases
(def store5 (store-new))
(insert store5 quad-default)
(insert store5 quad-named)
(remove store5 quad-default)
(assert-false
  (contains store5 quad-default)
  "contains returns false after remove")
(assert-eq
  (length (quads store5))
  1
  "quads length decreases by 1 after remove")

## remove non-existent quad is a no-op (no error)
(def store6 (store-new))
(remove store6 quad-default)
(assert-eq
  (length (quads store6))
  0
  "remove of non-existent quad leaves store unchanged")

## structural verification: quad array elements are term arrays
(def store7 (store-new))
(insert store7 quad-default)
(def result-quads (quads store7))
(def q (get result-quads 0))

## subject: [:iri "http://example.org/alice"]
(assert-eq
  (get q 0)
  [:iri "http://example.org/alice"]
  "quad element 0 (subject) is [:iri ...] array")

## predicate: [:iri "http://xmlns.com/foaf/0.1/name"]
(assert-eq
  (get q 1)
  [:iri "http://xmlns.com/foaf/0.1/name"]
  "quad element 1 (predicate) is [:iri ...] array")

## object: [:literal "Alice"]
(assert-eq
  (get q 2)
  [:literal "Alice"]
  "quad element 2 (object) is [:literal ...] array")

## graph-name: nil (default graph)
(assert-eq
  (get q 3)
  nil
  "quad element 3 (graph-name) is nil for default graph")
