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
