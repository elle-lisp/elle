# elle-oxigraph

An RDF quad store plugin for Elle, wrapping the Rust `oxigraph` crate. Provides in-memory and persistent storage with SPARQL query and update.

## Building

Built as part of the workspace:

```sh
cargo build --workspace
```

Produces `target/debug/libelle_oxigraph.so` (or `target/release/libelle_oxigraph.so`).

## Data model

### RDF terms as arrays

Every RDF term is an immutable array with a keyword tag as the first element.

**IRI:**
```lisp
[:iri "http://example.org/alice"]
```

**Blank node:**
```lisp
[:bnode "b0"]
```

**Literal (plain):**
```lisp
[:literal "hello"]
```

**Literal (language-tagged):**
```lisp
[:literal "hello" :lang "en"]
```

**Literal (typed):**
```lisp
[:literal "42" :datatype "http://www.w3.org/2001/XMLSchema#integer"]
```

### Quads

A quad is a 4-element array `[subject predicate object graph-name]`. The graph-name is a term or `nil` for the default graph.

```lisp
[[:iri "http://ex.org/alice"]
 [:iri "http://xmlns.com/foaf/0.1/name"]
 [:literal "Alice"]
 nil]
```

### Query results

SELECT queries return an array of structs. Each struct has keyword keys matching SPARQL variable names (without `?`). Values are term arrays. Unbound variables are omitted.

```lisp
{:s [:iri "http://example.org/alice"]
 :p [:iri "http://xmlns.com/foaf/0.1/name"]
 :o [:literal "Alice"]}
```

## Primitives

### Store lifecycle

| Name | Args | Returns | Description |
|------|------|---------|-------------|
| `oxigraph/store-new` | — | store | Create in-memory RDF store |
| `oxigraph/store-open` | path | store | Open persistent on-disk store |

### Quad CRUD

| Name | Args | Returns | Description |
|------|------|---------|-------------|
| `oxigraph/insert` | store, quad | nil | Insert a quad |
| `oxigraph/remove` | store, quad | nil | Remove a quad (no-op if absent) |
| `oxigraph/contains` | store, quad | bool | Check if quad exists |
| `oxigraph/quads` | store | array | Return all quads as immutable array |

### SPARQL

| Name | Args | Returns | Description |
|------|------|---------|-------------|
| `oxigraph/query` | store, sparql | array/bool | Execute SELECT/CONSTRUCT/ASK query |
| `oxigraph/update` | store, sparql-update | nil | Execute SPARQL UPDATE |

### Serialization

| Name | Args | Returns | Description |
|------|------|---------|-------------|
| `oxigraph/load` | store, data, format | nil | Load RDF data (`:turtle`, `:ntriples`, `:nquads`, `:rdfxml`) |
| `oxigraph/dump` | store, format | string | Serialize store to string |

**`oxigraph/dump` format behavior:**
- `:nquads` — dumps all graphs (named and default). Use this to preserve named graph information.
- `:turtle`, `:ntriples`, `:rdfxml` — dumps the **default graph only**. Named graphs are omitted. These are triple-only formats that do not support multiple graphs.

### Term constructors

| Name | Args | Returns | Description |
|------|------|---------|-------------|
| `oxigraph/iri` | iri-string | `[:iri "..."]` | Construct and validate IRI |
| `oxigraph/literal` | value [tag-key tag-value] | `[:literal ...]` | Construct literal (plain, language-tagged, or typed) |
| `oxigraph/blank-node` | [id] | `[:bnode "..."]` | Construct blank node (auto-generated or explicit ID) |

## Usage example

```lisp
(import-file "target/release/libelle_oxigraph.so")

(def store (oxigraph/store-new))

(def alice (oxigraph/iri "http://example.org/alice"))
(def name-pred (oxigraph/iri "http://xmlns.com/foaf/0.1/name"))
(def alice-name (oxigraph/literal "Alice"))

(oxigraph/insert store [alice name-pred alice-name nil])

(oxigraph/query store "SELECT ?s ?o WHERE { ?s <http://xmlns.com/foaf/0.1/name> ?o }")
;; => [{:s [:iri "http://example.org/alice"] :o [:literal "Alice"]}]

(oxigraph/dump store :ntriples)
;; => "<http://example.org/alice> <http://xmlns.com/foaf/0.1/name> \"Alice\" .\n"
```

## Error kinds

| Kind | When |
|------|------|
| `type-error` | Wrong argument type, invalid array shape, unknown format keyword |
| `oxigraph-error` | Store creation/open failure, malformed IRI, invalid blank node ID, invalid language tag, store operation failure |
| `sparql-error` | SPARQL parse or evaluation error |
