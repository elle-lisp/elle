#!/usr/bin/env elle
;; Elle Documentation Generator
;; Reads JSON input files and produces static HTML documentation

;; ============================================================================
;; HTML generation utilities
;; ============================================================================

;; Escape HTML special characters
(define html-escape
  (fn (str)
    ;; Handle nil or non-string values
    (if (nil? str)
      ""
      (if (not (string? str))
        (string str)
        ;; Apply all escapes in sequence
        (begin
          (define s1 (string-replace str "&" "&amp;"))
          (define s2 (string-replace s1 "<" "&lt;"))
          (define s3 (string-replace s2 ">" "&gt;"))
          (define s4 (string-replace s3 "\"" "&quot;"))
          (string-replace s4 "'" "&#39;"))))))

;; Find delimiter in text starting from character position
(define find-closing-helper (fn (text pos tlen delimiter dlen)
  (if (> (+ pos dlen) tlen)
    nil
    (if (= (substring text pos (+ pos dlen)) delimiter)
      pos
      (find-closing-helper text (+ pos 1) tlen delimiter dlen)))))

(define find-closing (fn (text start delimiter)
  (find-closing-helper text start (length text) delimiter (length delimiter))))

;; Convert markdown links [text](url) to HTML anchor tags
(define format-links-rec (fn (remaining result)
  (if (= (length remaining) 0)
    result
    (begin
      (define open-bracket (find-closing remaining 0 "["))
      (if (nil? open-bracket)
        (string-append result remaining)
        (begin
          (define before (if (> open-bracket 0) (substring remaining 0 open-bracket) ""))
          (define mid (find-closing remaining (+ open-bracket 1) "]("))
           (if (nil? mid)
             (format-links-rec
               (substring remaining (+ open-bracket 1) (length remaining))
               (string-append result before "["))
            (begin
              (define close-paren (find-closing remaining (+ mid 2) ")"))
               (if (nil? close-paren)
                 (format-links-rec
                   (substring remaining (+ open-bracket 1) (length remaining))
                   (string-append result before "["))
                 (begin
                   (define link-text (substring remaining (+ open-bracket 1) mid))
                   (define link-url (substring remaining (+ mid 2) close-paren))
                   (define after (substring remaining (+ close-paren 1) (length remaining)))
                   (format-links-rec
                    after
                    (string-append result before "<a href=\"" link-url "\">" link-text "</a>"))))))))))))

(define format-links (fn (text)
  (format-links-rec text "")))

;; Format inline markdown: **bold**, *italic*, `code`
;; Uses string-split to avoid UTF-8 boundary issues
(define format-inline (fn (text)
  (begin
    ;; First, escape HTML characters
    (define escaped (html-escape text))
    
    ;; Then handle **bold** by splitting and rejoining
    (define parts-bold (string-split escaped "**"))
    (define format-bold
      (fn (parts result is-bold)
        (if (nil? parts)
          result
          (begin
            (define part (first parts))
            (define rest-parts (rest parts))
            (define new-result
              (if is-bold
                (string-append result "<strong>" part "</strong>")
                (string-append result part)))
            (format-bold rest-parts new-result (not is-bold))))))
    (define after-bold (format-bold parts-bold "" #f))
    
    ;; Then handle *italic* by splitting and rejoining
    (define parts-italic (string-split after-bold "*"))
    (define format-italic
      (fn (parts result is-italic)
        (if (nil? parts)
          result
          (begin
            (define part (first parts))
            (define rest-parts (rest parts))
            (define new-result
              (if is-italic
                (string-append result "<em>" part "</em>")
                (string-append result part)))
            (format-italic rest-parts new-result (not is-italic))))))
    (define after-italic (format-italic parts-italic "" #f))
    
    ;; Then handle `code` by splitting and rejoining
    (define parts-code (string-split after-italic "`"))
    (define format-code
      (fn (parts result is-code)
        (if (nil? parts)
          result
          (begin
            (define part (first parts))
            (define rest-parts (rest parts))
            (define new-result
              (if is-code
                (string-append result "<code>" part "</code>")
                (string-append result part)))
            (format-code rest-parts new-result (not is-code))))))
    (define after-code (format-code parts-code "" #f))
    
    (format-links after-code))))

;; ============================================================================
;; CSS stylesheet generation
;; ============================================================================

(define generate-css
  (fn ()
    "/* Elle Documentation Site Stylesheet */

:root {
  color-scheme: light dark;
  --bg: #ffffff;
  --fg: #1a1a2e;
  --bg-secondary: #f8f9fa;
  --code-bg: #f5f5f5;
  --code-fg: #e83e8c;
  --accent: #6c5ce7;
  --accent-hover: #5a4bd1;
  --border: #e0e0e0;
  --shadow: rgba(0,0,0,0.1);
  --note-info-bg: #e3f2fd;
  --note-info-border: #2196f3;
  --note-warning-bg: #fff3e0;
  --note-warning-border: #ff9800;
  --note-tip-bg: #e8f5e9;
  --note-tip-border: #4caf50;
}

@media (prefers-color-scheme: dark) {
  :root {
    --bg: #1a1a2e;
    --fg: #e0e0e0;
    --bg-secondary: #16213e;
    --code-bg: #2d2d44;
    --code-fg: #f78da7;
    --accent: #a29bfe;
    --accent-hover: #b8b3ff;
    --border: #3d3d5c;
    --shadow: rgba(0,0,0,0.3);
    --note-info-bg: #1a237e;
    --note-info-border: #5c6bc0;
    --note-warning-bg: #3e2723;
    --note-warning-border: #ff8f00;
    --note-tip-bg: #1b5e20;
    --note-tip-border: #66bb6a;
  }
}

* {
  margin: 0;
  padding: 0;
  box-sizing: border-box;
}

html {
  scroll-behavior: smooth;
}

body {
  font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif;
  background-color: var(--bg);
  color: var(--fg);
  line-height: 1.6;
  transition: background-color 0.3s ease, color 0.3s ease;
}

/* Layout */
body {
  display: flex;
  min-height: 100vh;
}

.sidebar {
  width: 250px;
  background-color: var(--bg-secondary);
  border-right: 1px solid var(--border);
  padding: 2rem 1rem;
  position: fixed;
  height: 100vh;
  overflow-y: auto;
  transition: background-color 0.3s ease;
}

.site-title {
  font-size: 1.5rem;
  font-weight: bold;
  margin-bottom: 2rem;
  color: var(--accent);
}

.sidebar ul {
  list-style: none;
}

.sidebar li {
  margin-bottom: 0.5rem;
}

.sidebar a {
  display: block;
  padding: 0.5rem 1rem;
  color: var(--fg);
  text-decoration: none;
  border-radius: 4px;
  transition: background-color 0.2s ease, color 0.2s ease;
}

.sidebar a:hover {
  background-color: var(--bg);
  color: var(--accent);
}

.sidebar a.active {
  background-color: var(--accent);
  color: white;
}

.content {
  margin-left: 250px;
  flex: 1;
  padding: 3rem;
  max-width: 900px;
}

/* Typography */
h1 {
  font-size: 2.5rem;
  margin-bottom: 1.5rem;
  margin-top: 0;
  color: var(--accent);
}

h2 {
  font-size: 2rem;
  margin-top: 2rem;
  margin-bottom: 1rem;
  color: var(--accent);
  border-bottom: 2px solid var(--border);
  padding-bottom: 0.5rem;
}

h3 {
  font-size: 1.5rem;
  margin-top: 1.5rem;
  margin-bottom: 0.75rem;
  color: var(--fg);
}

p {
  margin-bottom: 1rem;
}

/* Links */
a {
  color: var(--accent);
  text-decoration: none;
  transition: color 0.2s ease;
}

a:hover {
  color: var(--accent-hover);
  text-decoration: underline;
}

/* Code */
code {
  background-color: var(--code-bg);
  color: var(--code-fg);
  padding: 0.2em 0.4em;
  border-radius: 3px;
  font-family: 'Courier New', Courier, monospace;
  font-size: 0.9em;
}

pre {
  background-color: var(--code-bg);
  border: 1px solid var(--border);
  border-radius: 6px;
  padding: 1rem;
  overflow-x: auto;
  margin-bottom: 1rem;
}

pre code {
  background-color: transparent;
  color: var(--fg);
  padding: 0;
  border-radius: 0;
}

/* Lists */
ul, ol {
  margin-left: 2rem;
  margin-bottom: 1rem;
}

li {
  margin-bottom: 0.5rem;
}

/* Blockquotes */
blockquote {
  border-left: 4px solid var(--accent);
  padding-left: 1rem;
  margin-left: 0;
  margin-bottom: 1rem;
  color: var(--fg);
  font-style: italic;
}

/* Tables */
table {
  width: 100%;
  border-collapse: collapse;
  margin-bottom: 1rem;
  border: 1px solid var(--border);
}

thead {
  background-color: var(--bg-secondary);
}

th {
  padding: 0.75rem;
  text-align: left;
  font-weight: bold;
  border-bottom: 2px solid var(--border);
}

td {
  padding: 0.75rem;
  border-bottom: 1px solid var(--border);
}

tbody tr:nth-child(even) {
  background-color: var(--bg-secondary);
}

/* Notes/Callouts */
.note {
  padding: 1rem;
  margin-bottom: 1rem;
  border-left: 4px solid;
  border-radius: 4px;
  background-color: var(--bg-secondary);
}

.note-info {
  border-left-color: var(--note-info-border);
  background-color: var(--note-info-bg);
}

.note-warning {
  border-left-color: var(--note-warning-border);
  background-color: var(--note-warning-bg);
}

.note-tip {
  border-left-color: var(--note-tip-border);
  background-color: var(--note-tip-bg);
}

/* Responsive */
@media (max-width: 768px) {
  body {
    flex-direction: column;
  }
  
  .sidebar {
    width: 100%;
    height: auto;
    position: relative;
    border-right: none;
    border-bottom: 1px solid var(--border);
    padding: 1rem;
  }
  
  .site-title {
    margin-bottom: 1rem;
  }
  
  .content {
    margin-left: 0;
    padding: 1.5rem;
  }
  
  h1 {
    font-size: 1.75rem;
  }
  
  h2 {
    font-size: 1.5rem;
  }
  
  h3 {
    font-size: 1.25rem;
  }
}

/* Utility */
.container {
  max-width: 900px;
  margin: 0 auto;
}
"))

;; ============================================================================
;; Content block rendering
;; ============================================================================

;; Render a paragraph block
(define render-paragraph
  (fn (block)
    (begin
      (define text (get block "text"))
      (string-append "<p>" (format-inline text) "</p>"))))

;; Render a code block
(define render-code
  (fn (block)
    (begin
      (define text (get block "text"))
      (define language (get block "language"))
      (string-append 
        "<pre><code class=\"language-" (html-escape language) "\">"
        (html-escape text)
        "</code></pre>"))))

;; Render a list block using fold
(define render-list
  (fn (block)
    (begin
      (define items (get block "items"))
      (define ordered (get block "ordered"))
      (define tag (if ordered "ol" "ul"))
      ;; Use fold to concatenate all rendered list items
      (define rendered-items
        (fold
          (fn (acc item)
            (string-append acc "<li>" (format-inline item) "</li>"))
          ""
          items))
      (string-append 
        "<" tag ">"
        rendered-items
        "</" tag ">"))))

;; Render a blockquote block
(define render-blockquote
  (fn (block)
    (begin
      (define text (get block "text"))
      (string-append "<blockquote>" (format-inline text) "</blockquote>"))))

;; Render a table block using fold
(define render-table
  (fn (block)
    (begin
      (define headers (get block "headers"))
      (define rows (get block "rows"))
      
      ;; Render header cells using fold
      (define rendered-headers
        (fold
          (fn (acc header)
            (string-append acc "<th>" (html-escape header) "</th>"))
          ""
          headers))
      
      ;; Render row cells using fold
      (define render-row-cells
        (fn (cells)
          (fold
            (fn (acc cell)
              (string-append acc "<td>" (html-escape cell) "</td>"))
            ""
            cells)))
      
      ;; Render all rows using fold
      (define rendered-rows
        (fold
          (fn (acc row)
            (string-append acc "<tr>" (render-row-cells row) "</tr>"))
          ""
          rows))
      
      (string-append 
        "<table><thead><tr>"
        rendered-headers
        "</tr></thead><tbody>"
        rendered-rows
        "</tbody></table>"))))

;; Render a note/callout block
(define render-note
  (fn (block)
    (begin
      (define text (get block "text"))
      (define kind (get block "kind"))
      (string-append 
        "<div class=\"note note-" (html-escape kind) "\">"
        (format-inline text)
        "</div>"))))

;; Main dispatcher
(define render-block
  (fn (block)
    (begin
      (define type (get block "type"))
      (cond
        ((string-contains? type "paragraph") (render-paragraph block))
        ((string-contains? type "code") (render-code block))
        ((string-contains? type "list") (render-list block))
        ((string-contains? type "blockquote") (render-blockquote block))
        ((string-contains? type "table") (render-table block))
        ((string-contains? type "note") (render-note block))
        ((string-contains? type "heading") "")
        (#t "")))))

;; Render blocks in a section
(define render-blocks-in-section
  (fn (blocks result)
    (if (nil? blocks)
      result
      (begin
        (define block (first blocks))
        (define rest-blocks (rest blocks))
        (render-blocks-in-section rest-blocks
          (string-append result (render-block block)))))))

;; Render a heading block (nested heading within content)
(define render-heading
  (fn (block)
    (begin
      (define level (get block "level"))
      (define content (get block "content"))
      (define level-str (number->string level))
      (string-append 
        "<h" level-str ">" (render-blocks-in-section content "") "</h" level-str ">"))))

;; Render a section with heading and content blocks
(define render-section
  (fn (section)
    (begin
      (define heading (get section "heading"))
      (define level (get section "level"))
      (define content (get section "content"))
      (define level-str (number->string level))
      (string-append 
        "<h" level-str ">" (html-escape heading) "</h" level-str ">"
        (render-blocks-in-section content "")))))

;; Render all sections using fold
(define render-sections
  (fn (sections)
    (fold
      (fn (acc section)
        (string-append acc (render-section section)))
      ""
      sections)))

;; ============================================================================
;; Page template generation
;; ============================================================================

;; Render navigation items
(define render-nav-items
  (fn (items current-slug result)
    (if (nil? items)
      result
      (begin
        (define item (first items))
        (define rest-items (rest items))
        (define nav-slug (get item "slug"))
        (define title (get item "title"))
        (define active-class (if (string-contains? nav-slug current-slug) " active" ""))
        (render-nav-items rest-items current-slug
          (string-append result 
            "<li><a href=\"" nav-slug ".html\" class=\"nav-link" active-class "\">" 
            title "</a></li>"))))))

;; Generate navigation HTML
(define generate-nav
  (fn (nav-items current-slug)
    (render-nav-items nav-items current-slug "")))

;; Generate the full HTML page
(define generate-page
  (fn (site page nav css body)
    (begin
      (define site-title (get site "title"))
      (define page-title (get page "title"))
      (define page-desc (get page "description"))
      (define nav-items (get site "nav"))
      (define current-slug (get page "slug"))
      (string-append
        "<!DOCTYPE html>\n"
        "<html lang=\"en\">\n"
        "<head>\n"
        "  <meta charset=\"UTF-8\">\n"
        "  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n"
        "  <title>" page-title " - " site-title "</title>\n"
        "  <meta name=\"description\" content=\"" page-desc "\">\n"
        "  <link rel=\"stylesheet\" href=\"style.css\">\n"
        "</head>\n"
        "<body>\n"
        "  <nav class=\"sidebar\">\n"
        "    <div class=\"site-title\">" site-title "</div>\n"
        "    <ul>\n"
        (generate-nav nav-items current-slug)
        "    </ul>\n"
        "  </nav>\n"
        "  <main class=\"content\">\n"
        "    <h1>" page-title "</h1>\n"
        body
        "  </main>\n"
        "</body>\n"
        "</html>\n"))))

;; ============================================================================
;; Main generator
;; ============================================================================

;; Configuration
(define docs-dir "elle-doc/docs")
(define output-dir "site")

;; Create output directory if it doesn't exist
(if (not (directory? output-dir))
  (create-directory-all output-dir))

;; Read and parse site configuration
(display "Reading site configuration...")
(newline)
(define site-json (slurp (join-path docs-dir "site.json")))
(define site-config (json-parse site-json))

;; Generate and write CSS
(display "Generating CSS...")
(newline)
(define css-content (generate-css))
(spit (join-path output-dir "style.css") css-content)

;; Get navigation items
(define nav-items (get site-config "nav"))

;; Process each page
(define process-pages
  (fn (all-nav-items current-nav-items)
    (if (nil? current-nav-items)
      (begin
        (display "Done!")
        (newline))
      (begin
        (define nav-item (first current-nav-items))
        (define rest-nav-items (rest current-nav-items))
        (define slug (get nav-item "slug"))
        (define title (get nav-item "title"))
        (define page-file (join-path docs-dir (string-append "pages/" slug ".json")))
        
        (display "Generating: ")
        (display slug)
        (display ".html...")
        (newline)
        
        (define page-json (slurp page-file))
        (define page-data (json-parse page-json))
        
        ;; Add slug to page data for template
        (put page-data "slug" slug)
        
        ;; Render content sections
        (define sections (get page-data "sections"))
        (define body-html (render-sections sections))
        
        ;; Generate full page (use all-nav-items for navigation)
        ;; NOTE: We pass slug as a separate parameter to avoid closure issues
        (define full-html (generate-page site-config page-data all-nav-items css-content body-html))
        
        ;; Write HTML file
        (define output-file (join-path output-dir (string-append slug ".html")))
        (spit output-file full-html)
        
        ;; Process next page
        (process-pages all-nav-items rest-nav-items)))))

(process-pages nav-items nav-items)

;; Print summary
(display "Generated documentation in ")
(display output-dir)
(display "/")
(newline)
