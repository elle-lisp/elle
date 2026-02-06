;; Content block rendering

(import-file "elle-doc/lib/html.l")

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
      (define render-items
        (lambda (items result)
          (if (empty? items)
            result
            (let ((item (first items)))
              (render-items (rest items)
                (string-append result "<li>" (format-inline item) "</li>"))))))
      (string-append 
        "<" tag ">"
        (render-items items "")
        "</" tag ">"))))

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
      (define render-header-cells
        (lambda (headers result)
          (if (empty? headers)
            result
            (let ((header (first headers)))
              (render-header-cells (rest headers)
                (string-append result "<th>" (html-escape header) "</th>"))))))
      
      (define render-row-cells
        (lambda (cells result)
          (if (empty? cells)
            result
            (let ((cell (first cells)))
              (render-row-cells (rest cells)
                (string-append result "<td>" (html-escape cell) "</td>"))))))
      
      (define render-rows
        (lambda (rows result)
          (if (empty? rows)
            result
            (let ((row (first rows)))
              (render-rows (rest rows)
                (string-append result 
                  "<tr>" (render-row-cells row "") "</tr>"))))))
      
      (string-append 
        "<table><thead><tr>"
        (render-header-cells headers "")
        "</tr></thead><tbody>"
        (render-rows rows "")
        "</tbody></table>"))))

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
      (define render-blocks
        (lambda (blocks result)
          (if (empty? blocks)
            result
            (let ((block (first blocks)))
              (render-blocks (rest blocks)
                (string-append result (render-block block)))))))
      
      (string-append 
        "<h" level-str ">" (html-escape heading) "</h" level-str ">"
        (render-blocks content "")))))

;; Render all sections
(define render-sections
  (lambda (sections)
    (define render-all
      (lambda (sections result)
        (if (empty? sections)
          result
          (let ((section (first sections)))
            (render-all (rest sections)
              (string-append result (render-section section)))))))
    (render-all sections "")))
