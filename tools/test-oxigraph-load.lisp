#!/usr/bin/env elle
## Quick check that the oxigraph plugin loads and works

(elle/epoch 5)
(def ox (import "oxigraph"))
(def store (ox:store-new))
(ox:update store "INSERT DATA { <http://example.org/a> <http://example.org/b> \"hello\" . }")
(def results (ox:query store "SELECT ?s ?p ?o WHERE { ?s ?p ?o }"))
(println results)
(println "oxigraph plugin OK")
