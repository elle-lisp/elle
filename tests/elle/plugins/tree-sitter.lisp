(elle/epoch 6)
## tree-sitter plugin integration tests

## Try to load the plugin. If it fails, exit cleanly.
(def [ok? ts] (protect (import-native "tree_sitter")))
(when (not ok?)
  (print "SKIP: tree-sitter plugin not built\n")
  (exit 0))

## ── ts/language ───────────────────────────────────────────────

(def c-lang (ts:language "c"))
(def rust-lang (ts:language "rust"))

(def [ok? _] (protect (ts:language "nope")))
(assert (not ok?) "ts/language rejects unknown language")

(def [ok? _] (protect (ts:language 42)))
(assert (not ok?) "ts/language rejects non-string")

## ── ts/parse + ts/root + ts/node-type ─────────────────────────

(def c-src "int main() { return 0; }")
(def tree (ts:parse c-src c-lang))
(def root (ts:root tree))

(assert (= (ts:node-type root) "translation_unit") "root node type")

## ── ts/node-count ─────────────────────────────────────────────

(assert (> (ts:node-count tree) 0) "node count > 0")

## ── ts/node-text ──────────────────────────────────────────────

(assert (= (ts:node-text root) c-src) "root node text is full source")

## ── ts/node-named? ────────────────────────────────────────────

(assert (ts:node-named? root) "root is a named node")

## ── ts/children + ts/named-children ───────────────────────────

(def children (ts:children root))
(assert (= (length children) 1) "root has 1 child")

(def named (ts:named-children root))
(assert (= (length named) 1) "root has 1 named child")

(def func-def (first named))
(assert (= (ts:node-type func-def) "function_definition") "child is function_definition")

## ── ts/child-by-field ─────────────────────────────────────────

(def body (ts:child-by-field func-def "body"))
(assert (not (nil? body)) "function_definition has body field")
(assert (= (ts:node-type body) "compound_statement") "body is compound_statement")

(def missing (ts:child-by-field func-def "nonexistent"))
(assert (nil? missing) "missing field returns nil")

## ── ts/parent ─────────────────────────────────────────────────

(def parent-node (ts:parent func-def))
(assert (= (ts:node-type parent-node) "translation_unit") "parent is translation_unit")
(assert (nil? (ts:parent root)) "parent of root is nil")

## ── ts/node-range ─────────────────────────────────────────────

(def range (ts:node-range root))
(assert (= (get range :start-byte) 0) "range start-byte is 0")
(assert (= (get range :start-row) 0) "range start-row is 0")
(assert (= (get range :start-col) 0) "range start-col is 0")
(assert (> (get range :end-byte) 0) "range end-byte > 0")

## ── ts/node-sexp ──────────────────────────────────────────────

(def sexp (ts:node-sexp root))
(assert (string? sexp) "node-sexp returns string")
(assert (string/starts-with? sexp "(translation_unit") "sexp starts with translation_unit")

## ── ts/query + ts/matches ─────────────────────────────────────

(def multi-src "int foo() { return 1; } int bar() { return 2; }")
(def multi-tree (ts:parse multi-src c-lang))
(def multi-root (ts:root multi-tree))

(def q (ts:query c-lang "(function_declarator declarator: (identifier) @fn-name)"))
(def matches (ts:matches q multi-root))

(assert (= (length matches) 2) "query finds 2 functions")

(def m0 (first matches))
(assert (= (get m0 :pattern) 0) "match pattern index is 0")
(assert (= (ts:node-text (get (get m0 :captures) :fn-name)) "foo") "first match is foo")

(def m1 (first (rest matches)))
(assert (= (ts:node-text (get (get m1 :captures) :fn-name)) "bar") "second match is bar")

## ── ts/captures ───────────────────────────────────────────────

(def flat-caps (ts:captures q multi-root))
(assert (= (length flat-caps) 2) "captures returns 2 entries")

(def c0 (first flat-caps))
(assert (= (get c0 :name) "fn-name") "capture name is fn-name")
(assert (= (ts:node-text (get c0 :node)) "foo") "first capture is foo")

## ── ts/query error ────────────────────────────────────────────

(def [ok? _] (protect (ts:query c-lang "(nonexistent_node @x)")))
(assert (not ok?) "ts/query rejects bad pattern")

## ── Rust parsing ──────────────────────────────────────────────

(def rust-src "fn greet(name: &str) -> String { name.to_string() }")
(def rust-tree (ts:parse rust-src rust-lang))
(def rust-root (ts:root rust-tree))

(assert (= (ts:node-type rust-root) "source_file") "Rust root is source_file")

(def rq (ts:query rust-lang "(function_item name: (identifier) @name)"))
(def rust-caps (ts:captures rq rust-root))
(assert (= (length rust-caps) 1) "Rust query finds 1 function")
(assert (= (ts:node-text (get (first rust-caps) :node)) "greet") "Rust fn name is greet")
