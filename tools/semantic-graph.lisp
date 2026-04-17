#!/usr/bin/env elle
## semantic-graph.lisp — extract semantic RDF triples from Elle source files
##
## Uses compile/analyze to emit triples enriched with signal inference,
## capture analysis, call graph edges, and composition properties.
## Delegates triple generation to lib/rdf.lisp.
##
## Usage:
##   elle tools/semantic-graph.lisp -- file1.lisp file2.lisp ...

(def rdf ((import "std/rdf/elle")))

(var args (drop 1 (sys/args)))

(when (empty? args)
  (println "usage: elle tools/semantic-graph.lisp -- file1.lisp ...")
  (exit 1))

# ── Main ────────────────────────────────────────────────────────────────

# Emit Rust primitives first so elle:calls edges resolve
(var out @"")
(push out (rdf:primitives))
(eprintln (string/format "emitted {} Rust primitives" (length (compile/primitives))))

(var file-count 0)

(each file in args
  (assign file-count (+ file-count 1))

  (let [[[read-ok? src] (protect (file/read file))]]
    (if (not read-ok?)
      (eprintln "  skip (read error): " file)
      (let [[[analyze-ok? analysis] (protect (compile/analyze src {:file file}))]]
        (if (not analyze-ok?)
          (eprintln "  skip (compile error): " file)
          (push out (rdf:file analysis file)))))))

(eprintln (string/format "extracted {} files" file-count))
(print (freeze out))
