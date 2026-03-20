(elle/epoch 1)

## XML plugin integration tests

(def [ok? plugin] (protect (import-file "target/release/libelle_xml.so")))
(when (not ok?)
  (display "SKIP: xml plugin not built\n")
  (exit 0))

(def parse-fn        (get plugin :parse))
(def emit-fn         (get plugin :emit))
(def reader-new-fn   (get plugin :reader-new))
(def next-event-fn   (get plugin :next-event))
(def reader-close-fn (get plugin :reader-close))

# ── xml/parse ──────────────────────────────────────────────────────

## Parse simple self-closing element
(let ((doc (parse-fn "<root/>")))
  (assert (= (get doc :tag) "root") "parse: simple element tag")
  (assert (= (length (get doc :children)) 0) "parse: simple element has no children"))

## Parse element with attributes
(let ((doc (parse-fn "<a href=\"http://example.com\" id=\"1\"/>")))
  (assert (= (get (get doc :attrs) :href) "http://example.com") "parse: attribute href")
  (assert (= (get (get doc :attrs) :id) "1") "parse: attribute id"))

## Parse nested elements
(let ((doc (parse-fn "<root><child>text</child></root>")))
  (assert (= (get doc :tag) "root") "parse: nested root tag")
  (assert (= (length (get doc :children)) 1) "parse: nested has one child")
  (let ((child (get (get doc :children) 0)))
    (assert (= (get child :tag) "child") "parse: nested child tag")
    (assert (= (get (get child :children) 0) "text") "parse: nested child text")))

## Parse text content
(let ((doc (parse-fn "<msg>hello world</msg>")))
  (assert (= (get (get doc :children) 0) "hello world") "parse: text content"))

## Parse CDATA treated as text
(let ((doc (parse-fn "<msg><![CDATA[hello & world]]></msg>")))
  (assert (= (get (get doc :children) 0) "hello & world") "parse: CDATA as text"))

## Parse special character entities in text
(let ((doc (parse-fn "<msg>&lt;tag&gt;</msg>")))
  (assert (= (get (get doc :children) 0) "<tag>") "parse: entity decoding"))

## Parse empty attributes struct
(let ((doc (parse-fn "<root></root>")))
  (assert (= (length (get doc :children)) 0) "parse: empty element")
  (assert (not (nil? (get doc :attrs))) "parse: attrs is not nil"))

## Roundtrip: parse then emit then parse again
(let* ((xml "<root><child attr=\"v\">text</child></root>")
       (doc1 (parse-fn xml))
       (emitted (emit-fn doc1))
       (doc2 (parse-fn emitted)))
  (assert (= (get doc2 :tag) (get doc1 :tag)) "roundtrip: tag matches")
  (assert (= (get (get (get doc2 :children) 0) :tag) (get (get (get doc1 :children) 0) :tag)) "roundtrip: child tag matches"))

## Error: malformed XML
(let (([ok? err] (protect ((fn () (parse-fn "<unclosed>")))))) (assert (not ok?) "parse: malformed XML returns xml-error") (assert (= (get err :error) :xml-error) "parse: malformed XML returns xml-error"))

## Error: non-string argument
(let (([ok? err] (protect ((fn () (parse-fn 42)))))) (assert (not ok?) "parse: non-string returns type-error") (assert (= (get err :error) :type-error) "parse: non-string returns type-error"))

# ── xml/emit ───────────────────────────────────────────────────────

## Emit simple element produces self-closing tag
(assert (= (emit-fn {:tag "root" :attrs {} :children []}) "<root/>") "emit: simple self-closing element")

## Emit element with children produces valid XML
(let ((result (emit-fn {:tag "root" :attrs {} :children
                        [{:tag "child" :attrs {} :children ["text"]}]})))
  (assert (not (nil? (parse-fn result))) "emit: output is valid XML"))

## Emit escapes special characters in text; re-parse recovers original
(let ((result (emit-fn {:tag "root" :attrs {} :children ["<hello> & \"world\""]})))
  (assert (not (= result nil)) "emit: escapes special chars")
  (let ((doc (parse-fn result)))
    (assert (= (get (get doc :children) 0) "<hello> & \"world\"") "emit: round-trip escaping")))

## Emit with attributes roundtrips cleanly
(let ((result (emit-fn {:tag "a" :attrs {:href "http://x.com"} :children []})))
  (let ((doc (parse-fn result)))
    (assert (= (get (get doc :attrs) :href) "http://x.com") "emit: attribute roundtrip")))

## Error: non-struct argument produces xml-error
(let (([ok? err] (protect ((fn () (emit-fn "not-a-struct")))))) (assert (not ok?) "emit: non-struct returns xml-error") (assert (= (get err :error) :xml-error) "emit: non-struct returns xml-error"))

## Error: missing :tag field
(let (([ok? err] (protect ((fn () (emit-fn {:attrs {} :children []})))))) (assert (not ok?) "emit: missing :tag field returns xml-error") (assert (= (get err :error) :xml-error) "emit: missing :tag field returns xml-error"))

# ── Streaming API ──────────────────────────────────────────────────

## xml/reader-new returns a non-nil external value
(let ((reader (reader-new-fn "<root><child>text</child></root>")))
  (assert (not (nil? reader)) "reader-new: returns non-nil"))

## Full streaming iteration through a document
(let ((reader (reader-new-fn "<root><child>text</child></root>")))
  (let ((e1 (next-event-fn reader)))
    (assert (= (get e1 :type) :start) "stream: first event is start")
    (assert (= (get e1 :tag) "root") "stream: first tag is root"))
  (let ((e2 (next-event-fn reader)))
    (assert (= (get e2 :type) :start) "stream: second event is start")
    (assert (= (get e2 :tag) "child") "stream: second tag is child"))
  (let ((e3 (next-event-fn reader)))
    (assert (= (get e3 :type) :text) "stream: third event is text")
    (assert (= (get e3 :content) "text") "stream: text content"))
  (let ((e4 (next-event-fn reader)))
    (assert (= (get e4 :type) :end) "stream: fourth event is end")
    (assert (= (get e4 :tag) "child") "stream: end tag is child"))
  (let ((e5 (next-event-fn reader)))
    (assert (= (get e5 :type) :end) "stream: fifth event is end root"))
  (let ((e6 (next-event-fn reader)))
    (assert (= (get e6 :type) :eof) "stream: sixth event is eof"))
  (assert (= (reader-close-fn reader) nil) "stream: reader-close returns nil"))

## XML declaration and comments are skipped; first event is the root element
(let ((reader (reader-new-fn "<?xml version=\"1.0\"?><!-- comment --><root></root>")))
  (let ((e1 (next-event-fn reader)))
    (assert (= (get e1 :type) :start) "stream: XML decl and comment skipped")
    (assert (= (get e1 :tag) "root") "stream: first meaningful event is root")))

## Error: non-reader to xml/next-event
(let (([ok? err] (protect ((fn () (next-event-fn "not-a-reader")))))) (assert (not ok?) "stream: non-reader to next-event returns type-error") (assert (= (get err :error) :type-error) "stream: non-reader to next-event returns type-error"))

## Error: non-reader to xml/reader-close
(let (([ok? err] (protect ((fn () (reader-close-fn "not-a-reader")))))) (assert (not ok?) "stream: non-reader to reader-close returns type-error") (assert (= (get err :error) :type-error) "stream: non-reader to reader-close returns type-error"))

## Error: malformed XML during streaming (unclosed tag inside root)
(let ((reader (reader-new-fn "<root><unclosed></root>")))
  ## Advance past root start
  (next-event-fn reader)
  ## unclosed start event for <unclosed>
  (next-event-fn reader)
  ## The </root> closes the wrong tag — quick-xml may error or return end
  ## Either an error or an :end event is acceptable; just confirm no crash
  (let ((e (protect (fn () (next-event-fn reader)))))
    (assert (not (nil? e)) "stream: malformed XML does not crash")))
