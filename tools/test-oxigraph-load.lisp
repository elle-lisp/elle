(elle/epoch 1)
#!/usr/bin/env elle
## Quick check that the oxigraph plugin loads and works

(def ox (import "target/release/libelle_oxigraph.so"))
(def store (ox:store-new))
(ox:update store "INSERT DATA { <http://example.org/a> <http://example.org/b> \"hello\" . }")
(def results (ox:query store "SELECT ?s ?p ?o WHERE { ?s ?p ?o }"))
(print results)
(print "oxigraph plugin OK")
