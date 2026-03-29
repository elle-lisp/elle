#!/usr/bin/env elle
## demo-queries.lisp — load elle graph into oxigraph and run SPARQL queries
## Run: elle demo-queries.lisp

(elle/epoch 5)
(def ox (import "oxigraph"))

# Fresh in-memory store for demo
(def store (ox:store-new))

# Load the generated ntriples
(def data (file/read "/tmp/elle-graph.nt"))
(ox:load store data :ntriples)
(println (string/format "Loaded {} triples.\n" (length (ox:query store "SELECT (COUNT(*) AS ?n) WHERE { ?s ?p ?o }"))))

# ── Queries ──────────────────────────────────────────────────────────

(defn heading [s]
  (println (string/format "── {} ──" s)))

(defn query [q]
  (json/pretty (ox:query store q)))

# 1. What types of things are in the graph?
(heading "Entity counts by type")
(println (query "SELECT ?type (COUNT(*) AS ?count)
               WHERE { ?s a ?type }
               GROUP BY ?type
               ORDER BY DESC(?count)"))

# 2. Which files define the most functions?
(heading "Functions per file (top 10)")
(println (query "SELECT ?file (COUNT(*) AS ?fns)
               WHERE { ?s a <urn:elle:Fn> ; <urn:elle:file> ?file }
               GROUP BY ?file
               ORDER BY DESC(?fns)
               LIMIT 10"))

# 3. What macros does Elle provide?
(heading "All macros")
(println (query "SELECT ?name ?file
               WHERE { ?s a <urn:elle:Macro> ; <urn:elle:name> ?name ; <urn:elle:file> ?file }
               ORDER BY ?name"))

# 4. Functions with docstrings
(heading "Documented functions (sample)")
(println (query "SELECT ?name ?doc
               WHERE { ?s a <urn:elle:Fn> ; <urn:elle:name> ?name ; <urn:elle:doc> ?doc }
               ORDER BY ?name
               LIMIT 10"))

# 5. What plugins are imported and where?
(heading "Plugin imports")
(println (query "SELECT ?name ?path ?file
               WHERE { ?s a <urn:elle:Import> ; <urn:elle:name> ?name ; <urn:elle:path> ?path ; <urn:elle:file> ?file }
               ORDER BY ?file"))

# 6. High-arity functions (complex APIs)
(heading "Functions with 3+ parameters")
(println (query "SELECT ?name ?arity ?file
               WHERE { ?s a <urn:elle:Fn> ; <urn:elle:name> ?name ; <urn:elle:arity> ?arity ; <urn:elle:file> ?file
                       FILTER(?arity >= \"3\") }
               ORDER BY DESC(?arity) ?name"))

# 7. What's defined in a specific example?
(heading "Everything in signals.lisp")
(println (query "SELECT ?type ?name
               WHERE { ?s a ?type ; <urn:elle:name> ?name ; <urn:elle:file> \"examples/signals.lisp\" }
               ORDER BY ?type ?name"))

# 8. Functions that share a name across files (potential collisions)
(heading "Names defined in multiple files")
(println (query "SELECT ?name (GROUP_CONCAT(DISTINCT ?file; separator=\", \") AS ?files) (COUNT(DISTINCT ?file) AS ?n)
               WHERE { ?s <urn:elle:name> ?name ; <urn:elle:file> ?file }
               GROUP BY ?name
               HAVING (COUNT(DISTINCT ?file) > 1)
               ORDER BY DESC(?n)"))

# 9. Undocumented public functions (missing docstrings)
(heading "Undocumented functions in stdlib.lisp")
(println (query "SELECT ?name
               WHERE {
                 ?s a <urn:elle:Fn> ; <urn:elle:name> ?name ; <urn:elle:file> \"stdlib.lisp\" .
                 FILTER NOT EXISTS { ?s <urn:elle:doc> ?doc }
               }
               ORDER BY ?name"))

# 10. Parameter usage — what names appear most?
(heading "Most common parameter names")
(println (query "SELECT ?param (COUNT(*) AS ?uses)
               WHERE { ?s a <urn:elle:Fn> ; <urn:elle:param> ?param }
               GROUP BY ?param
               ORDER BY DESC(?uses)
               LIMIT 15"))
