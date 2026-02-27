;; Content block rendering

(import-file "elle-doc/lib/html.lisp")

;; Render a paragraph block
(var render-paragraph
  (fn (block)
    (let ((text (get block "text")))
      (-> "<p>" (append (format-inline text)) (append "</p>")))))

;; Render a code block
(var render-code
  (fn (block)
    (let ((text (get block "text"))
          (language (get block "language")))
      (-> "<pre><code class=\"language-" (append (html-escape language)) (append "\">")
        (append (html-escape text))
        (append "</code></pre>")))))

;; Render a list block
(var render-list
  (fn (block)
    (let ((items (get block "items"))
          (ordered (get block "ordered"))
          (tag (if ordered "ol" "ul")))
      ;; Use fold to concatenate all rendered list items
      (let ((rendered-items
              (fold
                (fn (acc item)
                  (-> acc (append "<li>") (append (format-inline item)) (append "</li>")))
                ""
                items)))
        (-> "<" (append tag) (append ">")
          (append rendered-items)
          (append "</") (append tag) (append ">"))))))

;; Render a blockquote block
(var render-blockquote
  (fn (block)
    (let ((text (get block "text")))
      (-> "<blockquote>" (append (format-inline text)) (append "</blockquote>")))))

;; Render a table block
(var render-table
  (fn (block)
    (let ((headers (get block "headers"))
          (rows (get block "rows")))
      
      ;; Render header cells using fold
      (let ((rendered-headers
              (fold
                (fn (acc header)
                  (-> acc (append "<th>") (append (html-escape header)) (append "</th>")))
                ""
                headers)))
        
        ;; Render a single row of cells using fold
        (var render-row-cells
          (fn (cells)
            (fold
              (fn (acc cell)
                (-> acc (append "<td>") (append (html-escape cell)) (append "</td>")))
              ""
              cells)))
        
        ;; Render all rows using fold
        (let ((rendered-rows
                (fold
                  (fn (acc row)
                    (-> acc (append "<tr>") (append (render-row-cells row)) (append "</tr>")))
                  ""
                  rows)))
          
          (-> "<table><thead><tr>"
            (append rendered-headers)
            (append "</tr></thead><tbody>")
            (append rendered-rows)
            (append "</tbody></table>"))))))

;; Render a note/callout block
(var render-note
  (fn (block)
    (let ((text (get block "text"))
          (kind (get block "kind")))
      (-> "<div class=\"note note-" (append (html-escape kind)) (append "\">")
        (append (format-inline text))
        (append "</div>")))))

;; Main dispatcher
(var render-block
  (fn (block)
    (let ((type (get block "type")))
      (cond
        ((string-contains? type "paragraph") (render-paragraph block))
        ((string-contains? type "code") (render-code block))
        ((string-contains? type "list") (render-list block))
        ((string-contains? type "blockquote") (render-blockquote block))
        ((string-contains? type "table") (render-table block))
        ((string-contains? type "note") (render-note block))
        (true "")))))

;; Render a section with heading and content blocks
(var render-section
  (fn (section)
    (let ((heading (get section "heading"))
          (level (get section "level"))
          (content (get section "content")))
      (var level-str (number->string level))
      ;; Use fold to render all blocks
      (let ((rendered-content
              (fold
                (fn (acc block)
                  (append acc (render-block block)))
                ""
                content)))
        
        (-> "<h" (append level-str) (append ">") (append (html-escape heading)) (append "</h") (append level-str) (append ">")
          (append rendered-content))))))

;; Render all sections using fold
(var render-sections
  (fn (sections)
    (fold
      (fn (acc section)
        (append acc (render-section section)))
      ""
      sections)))
