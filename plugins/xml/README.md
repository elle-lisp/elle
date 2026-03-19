# elle-xml

XML parsing, serialization, and streaming for Elle, via the `quick-xml` crate.

## Building

Built as part of the workspace:

```sh
cargo build --workspace
```

Produces `target/debug/libelle_xml.so` (or `target/release/libelle_xml.so`).

## Usage

### DOM API (eager parsing)

```lisp
(import-file "path/to/libelle_xml.so")

;; Parse XML to nested structs
(def doc (xml/parse "<root><child attr=\"val\">text</child></root>"))
;; => {:tag "root" :attrs {} :children [{:tag "child" :attrs {:attr "val"} :children ["text"]}]}

;; Emit struct tree back to XML
(xml/emit doc)
;; => "<root><child attr=\"val\">text</child></root>"
```

### Streaming API (pull-based)

```lisp
(def reader (xml/reader-new "<root><child>text</child></root>"))

(xml/next-event reader)  ;; {:type :start :tag "root" :attrs {}}
(xml/next-event reader)  ;; {:type :start :tag "child" :attrs {}}
(xml/next-event reader)  ;; {:type :text :content "text"}
(xml/next-event reader)  ;; {:type :end :tag "child"}
(xml/next-event reader)  ;; {:type :end :tag "root"}
(xml/next-event reader)  ;; {:type :eof}
(xml/reader-close reader)
```

## Primitives

### High-level API

#### `xml/parse`

**Signature:** `(xml/parse xml-string)`

**Arguments:**
- `xml-string` — string (XML document)

**Returns:** element struct

**Signal:** errors

**Description:** Parse an XML string into a nested struct/array tree. The entire document is loaded into memory. Returns the root element.

**Example:**
```lisp
(xml/parse "<root><child>text</child></root>")
;; => {:tag "root" :attrs {} :children [{:tag "child" :attrs {} :children ["text"]}]}
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| `xml-string` is not a string | `type-error` | `"xml/parse: expected string, got {type}"` |
| XML is malformed | `xml-error` | `"xml/parse: {reason}"` |
| Document is empty | `xml-error` | `"xml/parse: empty document"` |

#### `xml/emit`

**Signature:** `(xml/emit element)`

**Arguments:**
- `element` — struct with `:tag`, `:attrs`, `:children` fields

**Returns:** string (XML)

**Signal:** errors

**Description:** Serialize an element struct tree to an XML string. Text children are XML-escaped. Empty elements are self-closed. Attributes are emitted in sorted order.

**Example:**
```lisp
(xml/emit {:tag "root" :attrs {:id "1"} :children ["hello"]})
;; => "<root id=\"1\">hello</root>"
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| `element` is not a struct | `type-error` | `"xml/emit: expected struct, got {type}"` |
| Missing `:tag` field | `xml-error` | `"xml/emit: missing field 'tag'"` |
| `:tag` is not a string | `xml-error` | `"xml/emit: field 'tag' must be a string, got {type}"` |
| Missing `:attrs` field | `xml-error` | `"xml/emit: missing field 'attrs'"` |
| `:attrs` is not a struct | `xml-error` | `"xml/emit: field 'attrs' must be a struct, got {type}"` |
| Attribute value is not a string | `xml-error` | `"xml/emit: attribute value for '{key}' must be a string, got {type}"` |
| Missing `:children` field | `xml-error` | `"xml/emit: missing field 'children'"` |
| `:children` is not an array | `xml-error` | `"xml/emit: field 'children' must be an array, got {type}"` |
| Document too deeply nested (>256 levels) | `xml-error` | `"xml/emit: document too deeply nested (max 256)"` |

### Streaming API

#### `xml/reader-new`

**Signature:** `(xml/reader-new xml-string)`

**Arguments:**
- `xml-string` — string (XML document)

**Returns:** xml-reader (external handle)

**Signal:** errors

**Description:** Create a streaming XML reader from a string. The reader is an opaque external value used with `xml/next-event` and `xml/reader-close`.

**Example:**
```lisp
(def reader (xml/reader-new "<root/>"))
;; => #<external:xml-reader>
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| `xml-string` is not a string | `type-error` | `"xml/reader-new: expected string, got {type}"` |

#### `xml/next-event`

**Signature:** `(xml/next-event reader)`

**Arguments:**
- `reader` — xml-reader (from `xml/reader-new`)

**Returns:** event struct

**Signal:** errors

**Description:** Read the next event from the streaming reader. Returns one of four event types: `:start` (opening tag), `:end` (closing tag), `:text` (text content), or `:eof` (end of document). Comments and processing instructions are silently skipped.

**Event shapes:**

- **Start tag:** `{:type :start :tag "name" :attrs {:key "val" ...}}`
- **End tag:** `{:type :end :tag "name"}`
- **Text:** `{:type :text :content "..."}`
- **End of file:** `{:type :eof}`

**Example:**
```lisp
(def reader (xml/reader-new "<root attr=\"val\">text</root>"))
(xml/next-event reader)  ;; {:type :start :tag "root" :attrs {:attr "val"}}
(xml/next-event reader)  ;; {:type :text :content "text"}
(xml/next-event reader)  ;; {:type :end :tag "root"}
(xml/next-event reader)  ;; {:type :eof}
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| `reader` is not an xml-reader | `type-error` | `"xml/next-event: expected xml-reader, got {type}"` |
| XML is malformed | `xml-error` | `"xml/next-event: {reason}"` |
| Attribute decoding fails | `xml-error` | `"xml/next-event: attribute decode: {reason}"` |
| Text decoding fails | `xml-error` | `"xml/next-event: text decode: {reason}"` |

#### `xml/reader-close`

**Signature:** `(xml/reader-close reader)`

**Arguments:**
- `reader` — xml-reader (from `xml/reader-new`)

**Returns:** nil

**Signal:** errors

**Description:** Close a streaming XML reader. This is optional cleanup; the reader is automatically freed when garbage collected. Returns nil.

**Example:**
```lisp
(def reader (xml/reader-new "<root/>"))
(xml/next-event reader)  ;; {:type :start :tag "root" :attrs {}}
(xml/reader-close reader)  ;; nil
```

**Error cases:**

| Condition | Error kind | Message |
|-----------|-----------|---------|
| `reader` is not an xml-reader | `type-error` | `"xml/reader-close: expected xml-reader, got {type}"` |

## Data Representation

### Element struct

An XML element is an immutable struct with three fields:

```lisp
{:tag "tagname"          ;; string: element name
 :attrs {:key "val"}     ;; struct: attribute keys and values (both strings)
 :children ["text" ...]} ;; array: strings (text nodes) and/or element structs
```

**Rules:**
- `:tag` is a string (element name, including namespace prefix if present)
- `:attrs` is a struct with string keys and string values
- `:children` is an array containing strings and/or element structs
- Empty attributes: `{}` (empty struct)
- Empty children: `[]` (empty array)
- Text nodes are plain strings in the `:children` array
- CDATA sections are treated as text (content extracted, wrapper discarded)
- Comments and processing instructions are discarded during parsing
- Namespaces are preserved as-is (e.g., `"xs:element"`); no expansion

### Event struct (streaming API)

Events returned by `xml/next-event` are immutable structs with a `:type` field:

**Start tag:**
```lisp
{:type :start
 :tag "name"
 :attrs {:key "val" ...}}
```

**End tag:**
```lisp
{:type :end
 :tag "name"}
```

**Text content:**
```lisp
{:type :text
 :content "..."}
```

**End of document:**
```lisp
{:type :eof}
```

## Worked Example

Parse and emit the same document using both APIs:

```lisp
(import-file "path/to/libelle_xml.so")

(def xml-str "<book id=\"1\"><title>Lisp</title><author>McCarthy</author></book>")

;; DOM API: parse to struct
(def doc (xml/parse xml-str))
;; => {:tag "book"
;;     :attrs {:id "1"}
;;     :children [{:tag "title" :attrs {} :children ["Lisp"]}
;;                {:tag "author" :attrs {} :children ["McCarthy"]}]}

;; Emit back to XML
(xml/emit doc)
;; => "<book id=\"1\"><title>Lisp</title><author>McCarthy</author></book>"

;; Streaming API: iterate events
(def reader (xml/reader-new xml-str))
(def events (list))

(let loop ()
  (def event (xml/next-event reader))
  (assign events (cons event events))
  (when (not (= (get event :type) :eof))
    (loop)))

(xml/reader-close reader)

;; events now contains all events in reverse order:
;; ({:type :eof}
;;  {:type :end :tag "author"}
;;  {:type :text :content "McCarthy"}
;;  {:type :start :tag "author" :attrs {}}
;;  {:type :end :tag "title"}
;;  {:type :text :content "Lisp"}
;;  {:type :start :tag "title" :attrs {}}
;;  {:type :start :tag "book" :attrs {:id "1"}})
```

## Limitations

- **Self-closing tags in streaming:** `<tag/>` in the streaming API emits a `:start` event only; no `:end` follows. Use explicit `<tag></tag>` in streaming contexts.
- **Comments and PIs:** Comments (`<!-- -->`) and processing instructions (`<?...?>`) are silently discarded.
- **Namespaces:** Not expanded. Namespace prefixes are preserved as-is in tag and attribute names (e.g., `"xs:element"`). Namespace expansion is a higher-level concern that can be built in Elle on top of this.
- **Memory:** `xml/parse` loads the entire document into memory. For multi-gigabyte XML files, use the streaming API (`xml/reader-new` + `xml/next-event`) instead.

## Summary table

| Name | Args | Returns | Signal |
|------|------|---------|--------|
| `xml/parse` | xml-string | element struct | errors |
| `xml/emit` | element struct | string | errors |
| `xml/reader-new` | xml-string | xml-reader (external) | errors |
| `xml/next-event` | reader | event struct | errors |
| `xml/reader-close` | reader | nil | errors |
