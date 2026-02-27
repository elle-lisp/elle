#!/usr/bin/env elle
## Elle Documentation Generator
## Reads JSON input files and produces static HTML documentation

## ============================================================================
## HTML generation utilities
## ============================================================================

## Escape HTML special characters
(var html-escape
  (fn (str)
    (if (nil? str)
      ""
      (if (not (string? str))
        (string str)
        (string-replace
          (string-replace
            (string-replace
              (string-replace
                (string-replace str "&" "&amp;")
                "<" "&lt;")
              ">" "&gt;")
            (string 34) "&quot;")
          "'" "&#39;")))))

## Find delimiter in text starting from character position
(def find-closing-helper (fn (text pos tlen delimiter dlen)
  (if (> (+ pos dlen) tlen)
    nil
    (if (= (substring text pos (+ pos dlen)) delimiter)
      pos
      (find-closing-helper text (+ pos 1) tlen delimiter dlen)))))

(def find-closing (fn (text start delimiter)
  (find-closing-helper text start (length text) delimiter (length delimiter))))

## Convert markdown links [text](url) to HTML anchor tags
## NOTE: Disabled for now due to compiler bug with define in nested contexts
(def format-links-rec (fn (remaining result)
  (append result remaining)))

(def format-links (fn (text)
  (format-links-rec text "")))

## Helper function to apply formatting to split parts using fold
## Applies a tag (like "strong", "em", "code") to alternating parts
(var apply-formatting
  (fn (parts tag)
    (first
      (fold
        (fn (state part)
          ## state is [result, is-active]
          ## Avoid variable definitions in fn to work around compiler bug
          (list
            (if (first (rest state))
              (-> (first state) (append "<") (append tag) (append ">") (append part) (append "</") (append tag) (append ">"))
               (append (first state) part))
            (not (first (rest state)))))
        (list "" false)
        parts))))

## Format inline markdown: **bold**, *italic*, `code`
## Uses string-split to avoid UTF-8 boundary issues
(def format-inline (fn (text)
  (format-links
    (apply-formatting
      (string-split
        (apply-formatting
          (string-split
            (apply-formatting
              (string-split
                (html-escape text)
                "**")
              "strong")
            "*")
          "em")
        "`")
      "code"))))

## ============================================================================
## CSS stylesheet generation
## ============================================================================

(var generate-css
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

## ============================================================================
## Content block rendering
## ============================================================================

## Render a paragraph block
(var render-paragraph
  (fn (block)
    (-> "<p>" (append (format-inline (get block "text"))) (append "</p>"))))

## Render a code block
(var render-code
  (fn (block)
    (-> "<pre><code class=\"language-" (append (html-escape (get block "language"))) (append "\">")
      (append (html-escape (get block "text")))
      (append "</code></pre>"))))

## Render a list block using fold
## NOTE: We call format-inline directly without storing in a variable
## to work around a compiler bug with variable definitions in fold closures
(var render-list
  (fn (block)
    (-> "<" (append (if (get block "ordered") "ol" "ul")) (append ">")
      (append (fold
        (fn (acc item)
          (-> acc (append "<li>") (append (format-inline item)) (append "</li>")))
        ""
        (get block "items")))
      (append "</") (append (if (get block "ordered") "ol" "ul")) (append ">"))))

## Render a blockquote block
(var render-blockquote
  (fn (block)
    (-> "<blockquote>" (append (format-inline (get block "text"))) (append "</blockquote>"))))

## Render a table block using fold
(var render-table
  (fn (block)
    (-> "<table><thead><tr>"
      (append (fold
        (fn (acc header)
          (-> acc (append "<th>") (append (html-escape header)) (append "</th>")))
        ""
        (get block "headers")))
      (append "</tr></thead><tbody>")
      (append (fold
        (fn (acc row)
          (-> acc (append "<tr>")
            (append (fold
              (fn (acc2 cell)
                (-> acc2 (append "<td>") (append (html-escape cell)) (append "</td>")))
              ""
              row))
            (append "</tr>")))
        ""
        (get block "rows")))
      (append "</tbody></table>"))))

## Render a note/callout block
(var render-note
  (fn (block)
    (-> "<div class=\"note note-" (append (html-escape (get block "kind"))) (append "\">")
      (append (format-inline (get block "text")))
      (append "</div>"))))

## Main dispatcher
(var render-block
  (fn (block)
    (cond
      ((string-contains? (get block "type") "paragraph") (render-paragraph block))
      ((string-contains? (get block "type") "code") (render-code block))
      ((string-contains? (get block "type") "list") (render-list block))
      ((string-contains? (get block "type") "blockquote") (render-blockquote block))
      ((string-contains? (get block "type") "table") (render-table block))
      ((string-contains? (get block "type") "note") (render-note block))
      ((string-contains? (get block "type") "heading") "")
      (true ""))))

## Render blocks in a section
(var render-blocks-in-section
  (fn (blocks result)
    (fold
      (fn (acc block)
        (append acc (render-block block)))
      result
      blocks)))

## Render a heading block (nested heading within content)
(var render-heading
  (fn (block)
    (-> "<h" (append (number->string (get block "level"))) (append ">")
      (append (render-blocks-in-section (get block "content") ""))
      (append "</h") (append (number->string (get block "level"))) (append ">"))))

## Render a section with heading and content blocks
(var render-section
  (fn (section)
    (-> "<h" (append (number->string (get section "level"))) (append ">")
      (append (html-escape (get section "heading")))
      (append "</h") (append (number->string (get section "level"))) (append ">")
      (append (render-blocks-in-section (get section "content") "")))))

## Render all sections using fold
## NOTE: We call render-section directly without storing in a variable
## to work around a compiler bug with variable definitions in fold closures
(var render-sections
  (fn (sections)
    (fold
      (fn (acc section)
        (append acc (render-section section)))
      ""
      sections)))

## ============================================================================
## Page template generation
## ============================================================================

## Render navigation items
(var render-nav-items
  (fn (items current-slug result)
    (fold
      (fn (acc item)
        (-> acc (append "<li><a href=\"") (append (get item "slug")) (append ".html\" class=\"nav-link")
          (append (if (string-contains? (get item "slug") current-slug) " active" ""))
          (append "\">") (append (get item "title")) (append "</a></li>")))
      result
      items)))

## Generate navigation HTML
(var generate-nav
  (fn (nav-items current-slug)
    (render-nav-items nav-items current-slug "")))

## Generate the full HTML page
(var generate-page
  (fn (site page nav css body)
    (-> "<!DOCTYPE html>\n"
      (append "<html lang=\"en\">\n")
      (append "<head>\n")
      (append "  <meta charset=\"UTF-8\">\n")
      (append "  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n")
      (append "  <title>") (append (get page "title")) (append " - ") (append (get site "title")) (append "</title>\n")
      (append "  <meta name=\"description\" content=\"") (append (get page "description")) (append "\">\n")
      (append "  <link rel=\"stylesheet\" href=\"style.css\">\n")
      (append "</head>\n")
      (append "<body>\n")
      (append "  <nav class=\"sidebar\">\n")
      (append "    <div class=\"site-title\">") (append (get site "title")) (append "</div>\n")
      (append "    <ul>\n")
      (append (generate-nav (get site "nav") (get page "slug")))
      (append "    </ul>\n")
      (append "  </nav>\n")
      (append "  <main class=\"content\">\n")
      (append "    <h1>") (append (get page "title")) (append "</h1>\n")
      (append body)
      (append "  </main>\n")
      (append "</body>\n")
      (append "</html>\n"))))

## ============================================================================
## Standard library reference generator (from runtime primitive metadata)
## ============================================================================

## Map category identifiers to display names
(var category-display-name
  (fn (cat)
    (cond
      ((= cat "") "Core")
      ((= cat "math") "Math")
      ((= cat "string") "String Operations")
      ((= cat "file") "File I/O")
      ((= cat "ffi") "FFI (Foreign Function Interface)")
      ((= cat "fiber") "Fibers")
      ((= cat "coro") "Coroutines")
      ((= cat "array") "Arrays")
      ((= cat "struct") "Structs")
      ((= cat "json") "JSON")
      ((= cat "clock") "Clock")
      ((= cat "time") "Time")
      ((= cat "meta") "Metaprogramming")
      ((= cat "debug") "Debugging")
      ((= cat "fn") "Function Introspection")
      ((= cat "os") "OS / Process")
      ((= cat "pkg") "Packages")
      ((= cat "module") "Modules")
      ((= cat "bit") "Bitwise Operations")
      (true cat))))

## Build a function signature string like "(cons car cdr)" from metadata
(var build-signature
  (fn (meta)
    (let* ((name (get meta :name))
           (params (get meta :params)))
      (if (empty? params)
        (-> "(" (append name) (append ")"))
        (-> "(" (append name) (append " ") (append (string-join params " ")) (append ")"))))))


## Build the description string, including aliases if any
(var build-description
  (fn (meta)
    (let* ((doc (get meta :doc))
           (aliases (get meta :aliases)))
      (if (empty? aliases)
        doc
        (-> doc (append " (alias: ") (append (string-join aliases ", ")) (append ")"))))))

## Group primitives by category, skipping aliases.
## Returns a table mapping category-name → list of metadata structs.
(var group-by-category
  (fn (names)
    (fold (fn (groups name)
            (let* ((meta (vm/primitive-meta name))
                   (canonical (get meta :name)))
              ## Skip aliases: name passed in doesn't match canonical name
              (if (not (= name canonical))
                groups
                (let* ((cat (get meta :category))
                       (existing (get groups cat))
                       (items (if (nil? existing) (list) existing)))
                  (put groups cat (append items (list meta)))
                  groups))))
          (table)
          names)))

## Build a section (table with heading) for one category
(var build-category-section
  (fn (cat-name metas)
    (let* ((rows (map (fn (meta)
                        (list (build-signature meta)
                              (build-description meta)
                              (get meta :example)))
                      metas))
           (tbl (table "type" "table"
                       "headers" (list "Function" "Description" "Example")
                       "rows" rows)))
      (table "heading" (category-display-name cat-name)
             "level" 2
             "content" (list tbl)))))

## Preferred category ordering for the stdlib reference page
(var category-order
  (list "" "math" "string" "array" "struct" "json"
        "file" "fn" "fiber" "coro" "clock" "time"
        "meta" "debug" "bit" "os" "pkg" "module" "ffi"))

## Generate all stdlib sections from runtime primitive metadata
(var generate-stdlib-sections
  (fn ()
    (let* ((names (vm/list-primitives))
           (groups (group-by-category names))
           (all-cats (keys groups)))
      ## Build sections in preferred order, then append any unknown categories
      (let* ((ordered
               (fold (fn (acc cat)
                       (let* ((metas (get groups cat)))
                         (if (nil? metas)
                           acc
                           (append acc (list (build-category-section cat metas))))))
                     (list)
                     category-order))
             ## Find categories not in category-order
             (extra
               (fold (fn (acc cat)
                       (if (fold (fn (found c) (or found (= c cat)))
                                 false
                                 category-order)
                         acc
                         (append acc (list (build-category-section
                                            cat (get groups cat))))))
                     (list)
                     all-cats)))
        (append ordered extra)))))

## ============================================================================
## Main generator
## ============================================================================

## Configuration
(var docs-dir "elle-doc/docs")
(var output-dir "site")

## Create output directory if it doesn't exist
(if (not (directory? output-dir))
  (create-directory-all output-dir))

## Read and parse site configuration
(display "Reading site configuration...")
(newline)
(var site-json (slurp (join-path docs-dir "site.json")))
(var site-config (json-parse site-json))

## Generate and write CSS
(display "Generating CSS...")
(newline)
(var css-content (generate-css))
(spit (join-path output-dir "style.css") css-content)

## Get navigation items
(var nav-items (get site-config "nav"))

## Process each page
(var process-pages
  (fn (all-nav-items current-nav-items)
    (if (empty? current-nav-items)
      (begin
        (display "Done!")
        (newline))
      (begin
        (var nav-item (first current-nav-items))
        (var rest-nav-items (rest current-nav-items))
        (var slug (get nav-item "slug"))
        (var title (get nav-item "title"))
        (var page-file (join-path docs-dir (-> "pages/" (append slug) (append ".json"))))
        
        (display "Generating: ")
        (display slug)
        (display ".html...")
        (newline)
        
        ## For stdlib-reference, generate from runtime metadata
        ## For all other pages, read from JSON
        (var page-data
          (if (= slug "stdlib-reference")
            (begin
              (var pd (table))
              (put pd "title" title)
              (put pd "description" "Built-in functions and operations in Elle")
              (put pd "sections" (generate-stdlib-sections))
              pd)
            (json-parse (slurp page-file))))
        
        ## Add slug to page data for template
        (put page-data "slug" slug)
        
        ## Render content sections
        (var sections (get page-data "sections"))
        (var body-html (render-sections sections))
        
        ## Generate full page (use all-nav-items for navigation)
        ## NOTE: We pass slug as a separate parameter to avoid closure issues
        (var full-html (generate-page site-config page-data all-nav-items css-content body-html))
        
        ## Write HTML file
        (var output-file (join-path output-dir (append slug ".html")))
        (spit output-file full-html)
        
        ## Process next page
        (process-pages all-nav-items rest-nav-items)))))

(process-pages nav-items nav-items)

## Print summary
(display "Generated documentation in ")
(display output-dir)
(display "/")
(newline)
