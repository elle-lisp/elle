;; Content block rendering

(import-file "elle-doc/lib/html.lisp")

;; Render a paragraph block
(define render-paragraph
  (lambda (block)
    (let ((text (get block "text")))
      (string-append "<p>" (format-inline text) "</p>"))))

;; Render a code block
(define render-code
  (lambda (block)
    (let ((text (get block "text"))
          (language (get block "language")))
      (string-append 
        "<pre><code class=\"language-" (html-escape language) "\">"
        (html-escape text)
        "</code></pre>"))))

;; Render a list block
(define render-list
  (lambda (block)
    (let ((items (get block "items"))
          (ordered (get block "ordered"))
          (tag (if ordered "ol" "ul")))
      ;; Use fold to concatenate all rendered list items
      (let ((rendered-items
              (fold
                (lambda (acc item)
                  (string-append acc "<li>" (format-inline item) "</li>"))
                ""
                items)))
        (string-append 
          "<" tag ">"
          rendered-items
          "</" tag ">"))))))

;; Render a blockquote block
(define render-blockquote
  (lambda (block)
    (let ((text (get block "text")))
      (string-append "<blockquote>" (format-inline text) "</blockquote>"))))

;; Render a table block
(define render-table
  (lambda (block)
    (let ((headers (get block "headers"))
          (rows (get block "rows")))
      
      ;; Render header cells using fold
      (let ((rendered-headers
              (fold
                (lambda (acc header)
                  (string-append acc "<th>" (html-escape header) "</th>"))
                ""
                headers)))
        
        ;; Render a single row of cells using fold
        (define render-row-cells
          (lambda (cells)
            (fold
              (lambda (acc cell)
                (string-append acc "<td>" (html-escape cell) "</td>"))
              ""
              cells)))
        
        ;; Render all rows using fold
        (let ((rendered-rows
                (fold
                  (lambda (acc row)
                    (string-append acc "<tr>" (render-row-cells row) "</tr>"))
                  ""
                  rows)))
          
          (string-append 
            "<table><thead><tr>"
            rendered-headers
            "</tr></thead><tbody>"
            rendered-rows
            "</tbody></table>"))))))

;; Render a note/callout block
(define render-note
  (lambda (block)
    (let ((text (get block "text"))
          (kind (get block "kind")))
      (string-append 
        "<div class=\"note note-" (html-escape kind) "\">"
        (format-inline text)
        "</div>"))))

;; Main dispatcher
(define render-block
  (lambda (block)
    (let ((type (get block "type")))
      (cond
        ((string-contains? type "paragraph") (render-paragraph block))
        ((string-contains? type "code") (render-code block))
        ((string-contains? type "list") (render-list block))
        ((string-contains? type "blockquote") (render-blockquote block))
        ((string-contains? type "table") (render-table block))
        ((string-contains? type "note") (render-note block))
        (#t "")))))

;; Render a section with heading and content blocks
(define render-section
  (lambda (section)
    (let ((heading (get section "heading"))
          (level (get section "level"))
          (content (get section "content")))
      (define level-str (number->string level))
      ;; Use fold to render all blocks
      (let ((rendered-content
              (fold
                (lambda (acc block)
                  (string-append acc (render-block block)))
                ""
                content)))
        
        (string-append 
          "<h" level-str ">" (html-escape heading) "</h" level-str ">"
          rendered-content)))))

;; Render all sections using fold
(define render-sections
  (lambda (sections)
    (fold
      (lambda (acc section)
        (string-append acc (render-section section)))
      ""
      sections)))
