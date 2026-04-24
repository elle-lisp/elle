(elle/epoch 9)
## Elle Documentation Generator
## Renders docs/*.md as HTML pages + auto-generated API reference
## with signal profiles from compile/analyze and portrait system.

# ── Configuration ──────────────────────────────────────────────────

(def @docs-root "docs")
(def @output-dir "site")
(def @docs-dir "demos/docgen/docs")
(def @github-base "https://github.com/anthropics/elle/blob/main")

# ── Imports ────────────────────────────────────────────────────────

(def md ((import "demos/docgen/lib/markdown.lisp")))
(def css-mod ((import "demos/docgen/lib/css.lisp")))

(def @html-escape md:html-escape)
(def @format-inline md:format-inline)
(def @parse-markdown md:parse)
(def @generate-css css-mod:generate-css)

# ── Signal display ─────────────────────────────────────────────────

(defn signal-class [bit]
  "CSS class for a signal keyword."
  (cond
    (= bit :error) "signal-error"
    (= bit :io)    "signal-io"
    (= bit :yield) "signal-yield"
    (= bit :exec)  "signal-exec"
    (= bit :ffi)   "signal-ffi"
    (= bit :fuel)  "signal-fuel"
    (= bit :wait)  "signal-wait"
    true           "signal-badge"))

(defn render-signal-badges [sig]
  "Render signal profile as HTML badges."
  (if (get sig :silent)
    "<span class=\"signal-badge signal-silent\">silent</span>"
    (let [bits (get sig :bits)]
      (if (empty? bits)
        ""
        (let [result @""]
          (each bit in (set->array bits)
            (push result (string "<span class=\"signal-badge "
              (signal-class bit) "\">"
              (string bit) "</span>")))
          (freeze result))))))

# ── Navigation ─────────────────────────────────────────────────────

(defn render-nav [nav-items current-slug]
  "Generate sidebar HTML from nav items."
  (def @html @"")
  (each item in nav-items
    (if (get item :section)
      (push html (string "      <li class=\"nav-section\">"
        (html-escape (get item :section)) "</li>\n"))
      (let* [slug (get item :slug)
             title (get item :title)
             active (if (= slug current-slug) " active" "")]
        (push html (string "      <li><a href=\"" slug ".html\" class=\"nav-link"
          active "\">" (html-escape title) "</a></li>\n")))))
  (freeze html))

# ── Page template ──────────────────────────────────────────────────

(defn generate-page [site-title page-title page-desc current-slug nav-items body]
  "Generate a complete HTML page."
  (string "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n"
    "  <meta charset=\"UTF-8\">\n"
    "  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n"
    "  <title>" (html-escape page-title) " - " (html-escape site-title) "</title>\n"
    "  <meta name=\"description\" content=\"" (html-escape page-desc) "\">\n"
    "  <link rel=\"stylesheet\" href=\"style.css\">\n"
    "</head>\n<body>\n"
    "  <nav class=\"sidebar\">\n"
    "    <div class=\"site-title\">" (html-escape site-title) "</div>\n"
    "    <ul>\n"
    (render-nav nav-items current-slug)
    "    </ul>\n"
    "  </nav>\n"
    "  <main class=\"content\">\n"
    "    <h1>" (html-escape page-title) "</h1>\n"
    body
    "  </main>\n"
    "</body>\n</html>\n"))

# ── Link rewriting ─────────────────────────────────────────────────

(defn rewrite-md-links [html source-dir slug-map]
  "Rewrite .md href links to .html using the slug map."
  (def @result @"")
  (def @pos 0)
  (def @len (length html))
  (while (< pos len)
    (let [found (string/find html "href=\"" pos)]
      (if (nil? found)
        (begin (push result (slice html pos len))
               (assign pos len))
        (let* [href-start (+ found 6)
               href-end (string/find html "\"" href-start)]
          (if (nil? href-end)
            (begin (push result (slice html pos len))
                   (assign pos len))
            (let [url (slice html href-start href-end)]
              (if (not (string/ends-with? url ".md"))
                # Not a .md link: keep as-is
                (begin
                  (push result (slice html pos (+ href-end 1)))
                  (assign pos (+ href-end 1)))
                # Rewrite .md link
                (let* [md-path (slice url 0 (- (length url) 3))
                       resolved (if (= source-dir "")
                                   md-path
                                   (cond
                                     (string/starts-with? md-path "../")
                                      (slice md-path 3 (length md-path))
                                     (string-contains? md-path "/") md-path
                                     true (string source-dir "/" md-path)))
                       slug (or (get slug-map resolved) resolved)]
                  (push result (slice html pos href-start))
                  (push result slug)
                  (push result ".html")
                  (assign pos href-end)))))))))
  (freeze result))

# ── API: Primitives ───────────────────────────────────────────────

(defn category-display-name [cat]
  (cond
    (= cat "") "Core"
    (= cat "math") "Math"
    (= cat "string") "String Operations"
    (= cat "file") "File I/O"
    (= cat "ffi") "FFI (Foreign Function Interface)"
    (= cat "fiber") "Fibers"
    (= cat "coro") "Coroutines"
    (= cat "array") "Arrays"
    (= cat "struct") "Structs"
    (= cat "json") "JSON"
    (= cat "clock") "Clock"
    (= cat "time") "Time"
    (= cat "meta") "Metaprogramming"
    (= cat "debug") "Debugging"
    (= cat "fn") "Function Introspection"
    (= cat "os") "OS / Process"
    (= cat "pkg") "Packages"
    (= cat "module") "Modules"
    (= cat "bit") "Bitwise Operations"
    true cat))

(def @category-order ["" "math" "string" "array" "struct" "json"
   "file" "fn" "fiber" "coro" "clock" "time"
   "meta" "debug" "bit" "os" "pkg" "module" "ffi"])

(defn generate-primitives-html []
  "Generate HTML for the primitives API page."
  (let* [names (vm/list-primitives)
         groups (@struct)]
    # Group by category, skipping aliases
    (each name in names
      (let* [meta (vm/primitive-meta name)
             canonical (get meta :name)]
        (when (= (string name) canonical)
          (let* [cat (get meta :category)
                 existing (get groups cat)
                 items (if (nil? existing) @[] existing)]
            (push items meta)
            (put groups cat items)))))

    (def @html @"")
    # Render in preferred order
    (each cat in category-order
      (let [metas (get groups cat)]
        (when (and (not (nil? metas)) (not (empty? metas)))
          (push html (string "<h2>" (category-display-name cat) "</h2>\n"))
          (push html "<table><thead><tr><th>Function</th><th>Description</th><th>Example</th></tr></thead><tbody>\n")
          (each meta in metas
            (let* [name (get meta :name)
                   params (get meta :params)
                   sig-str (if (empty? params)
                              (string "(" name ")")
                              (string "(" name " " (string/join params " ") ")"))
                   doc (get meta :doc)
                   aliases (get meta :aliases)
                   desc (if (empty? aliases)
                           doc
                           (string doc " (alias: " (string/join aliases ", ") ")"))
                   example (get meta :example)]
              (push html (string "<tr><td><code>" (html-escape sig-str) "</code></td>"
                "<td>" (html-escape desc) "</td>"
                "<td><code>" (html-escape (or example "")) "</code></td></tr>\n"))))
          (push html "</tbody></table>\n"))))

    # Handle any categories not in the preferred order
    (each [cat metas] in (pairs groups)
      (when (not (any? (fn [c] (= c cat)) category-order))
        (push html (string "<h2>" (category-display-name cat) "</h2>\n"))
        (push html "<table><thead><tr><th>Function</th><th>Description</th><th>Example</th></tr></thead><tbody>\n")
        (each meta in metas
          (let* [name (get meta :name)
                 params (get meta :params)
                 sig-str (if (empty? params)
                            (string "(" name ")")
                            (string "(" name " " (string/join params " ") ")"))
                 doc (get meta :doc)
                 example (get meta :example)]
            (push html (string "<tr><td><code>" (html-escape sig-str) "</code></td>"
              "<td>" (html-escape doc) "</td>"
              "<td><code>" (html-escape (or example "")) "</code></td></tr>\n"))))
        (push html "</tbody></table>\n")))

    (freeze html)))

# ── API: Prelude macros ───────────────────────────────────────────

(defn generate-prelude-html []
  "Generate HTML for prelude macros."
  (let [source (slurp "prelude.lisp")]
    (def @html @"")
    (push html "<p>Macros loaded automatically before user code. These expand at compile time.</p>\n")
    (push html "<table><thead><tr><th>Macro</th><th>Description</th></tr></thead><tbody>\n")

    (let [lines (string/split source "\n")]
      (def @comment-lines @[])
      (def @i 0)
      (def @n (length lines))
      (while (< i n)
        (let [line (get lines i)]
          (cond
            (string/starts-with? line "## ")
             (push comment-lines (slice line 3 (length line)))
            (string/starts-with? line "(defmacro ")
             (let* [after (slice line 10 (length line))
                    parts (string/split after " ")
                    name (get parts 0)
                    desc (string/join (freeze comment-lines) " ")
                    line-num (+ i 1)
                    source-link (string github-base "/prelude.lisp#L" (string line-num))]
               (push html (string "<tr><td><code>" (html-escape name) "</code>"
                 " <a class=\"source-link\" href=\"" source-link "\">src</a></td>"
                 "<td>" (format-inline desc) "</td></tr>\n"))
               (assign comment-lines @[]))
            true
             (when (not (= (string/trim line) ""))
               (assign comment-lines @[]))))
        (assign i (+ i 1))))

    (push html "</tbody></table>\n")
    (freeze html)))

# ── API: Stdlib functions with signal profiles ─────────────────────

(defn extract-section-name [line]
  "Extract section name from ## ── Name ── line."
  (when (string/starts-with? line "## ")
    (let [text (slice line 3 (length line))]
      (when (string-contains? text "──")
        (let* [parts (string/split text "──")
               name (string/trim (get parts 1))]
          (when (not (= name ""))
            name))))))

(defn generate-stdlib-html []
  "Generate HTML for stdlib functions with signal profiles."
  (let* [source (slurp "stdlib.lisp")
         analysis (compile/analyze source {:file "stdlib.lisp"})
         syms (compile/symbols analysis)
         fn-syms (filter (fn [s] (= (get s :kind) :function)) syms)
         lines (string/split source "\n")]

    (def @html @"")
    (push html "<p>Runtime functions loaded at startup after primitives.</p>\n")

    # Parse source to find section headers and defn lines
    (def @current-section nil)
    (def @sections @[])
    (def @section-fns @{})
    (def @fn-comments @{})
    (def @fn-lines @{})

    (def @comment-lines @[])
    (def @i 0)
    (def @n (length lines))
    (while (< i n)
      (let [line (get lines i)]
        (let [section-name (extract-section-name line)]
          (when section-name
            (assign current-section section-name)
            (unless (any? (fn [s] (= s section-name)) sections)
              (push sections section-name)
              (put section-fns section-name @[]))))

        (cond
          (string/starts-with? line "## ")
           (unless (extract-section-name line)
             (push comment-lines (slice line 3 (length line))))
          (string/starts-with? line "(defn ")
           (let* [after (slice line 6 (length line))
                  parts (string/split after " ")
                  name (get parts 0)]
             (when current-section
               (let [fns (get section-fns current-section)]
                 (when fns (push fns name))))
             (put fn-comments name (string/join (freeze comment-lines) " "))
             (put fn-lines name (+ i 1))
             (assign comment-lines @[]))
          (string/starts-with? line "(def ")
           (assign comment-lines @[])
          true
           (when (and (not (= (string/trim line) ""))
                      (not (string/starts-with? line "## ")))
             (assign comment-lines @[]))))
      (assign i (+ i 1)))

    # Render each section
    (each section-name in (freeze sections)
      (let [fns (get section-fns section-name)]
        (when (and fns (not (empty? fns)))
          (push html (string "<h2>" (html-escape section-name) "</h2>\n"))
          (each fn-name in (freeze fns)
            (let* [sig-result (protect (compile/signal analysis (keyword fn-name)))
                   sig (when (get sig-result 0) (get sig-result 1))
                   desc (or (get fn-comments fn-name) "")
                   line-num (get fn-lines fn-name)
                   source-link (when line-num
                                  (string github-base "/stdlib.lisp#L" (string line-num)))]
              (push html "<div class=\"api-entry\">")
              (push html (string "<code class=\"api-signature\">" (html-escape fn-name) "</code>"))
              (when source-link
                (push html (string " <a class=\"source-link\" href=\""
                  source-link "\">src</a>")))
              (when sig
                (push html (string "\n<div class=\"api-signals\">"
                  (render-signal-badges sig) "</div>")))
              (when (not (= desc ""))
                (push html (string "<p class=\"api-desc\">"
                  (format-inline desc) "</p>")))
              (push html "</div>\n"))))))

    (freeze html)))

# ── API: Libraries ────────────────────────────────────────────────

(defn generate-libraries-html []
  "Generate HTML for lib/*.lisp modules."
  (let* [result (subprocess/system "find" ["lib" "-maxdepth" "1" "-name" "*.lisp" "-type" "f"])
         files (filter (fn [f] (not (= f "")))
                  (string/split result:stdout "\n"))]
    (def @html @"")
    (push html "<p>Reusable libraries in <code>lib/</code>. Each exports a struct of functions.</p>\n")

    (each file in (sort-with (fn [a b] (if (< a b) -1 (if (> a b) 1 0))) files)
      (let* [source (slurp file)
             lines (string/split source "\n")
             # Extract first ## comment as description
             desc (let [@first-comment nil]
                     (each line in lines
                       (when (and (nil? first-comment) (string/starts-with? line "## "))
                         (assign first-comment (slice line 3 (length line)))))
                     first-comment)
             # Extract module name from filename
             basename (let* [parts (string/split file "/")
                              fname (get parts (- (length parts) 1))]
                         (slice fname 0 (- (length fname) 5)))
             # Try to analyze
             analysis-result (protect (compile/analyze source {:file file}))]

        (push html (string "<h2>" (html-escape basename)
          " <a class=\"source-link\" href=\"" github-base "/" file "\">source</a></h2>\n"))
        (when desc
          (push html (string "<p>" (format-inline desc) "</p>\n")))

        # Show exports with signal profiles if analysis succeeded
        (when (get analysis-result 0)
          (let* [analysis (get analysis-result 1)
                 syms-result (protect (compile/symbols analysis))]
            (when (get syms-result 0)
              (let* [syms (get syms-result 1)
                     fn-syms (filter (fn [s]
                                        (and (struct? s)
                                             (= (get s :kind) :function)))
                                syms)]
                (when (and (array? fn-syms) (not (empty? fn-syms)))
                  (push html "<table><thead><tr><th>Export</th><th>Signals</th></tr></thead><tbody>\n")
                  (each sym in fn-syms
                    (let* [name (get sym :name)
                           line-num (get sym :line)
                           sig-result (protect (compile/signal analysis (keyword name)))
                           badges (if (get sig-result 0)
                                     (render-signal-badges (get sig-result 1))
                                     "")]
                      (push html (string "<tr><td><code>" (html-escape name) "</code>"
                        (if line-num
                          (string " <a class=\"source-link\" href=\""
                            github-base "/" file "#L" (string line-num) "\">src</a>")
                          "")
                        "</td><td>" badges "</td></tr>\n"))))
                  (push html "</tbody></table>\n"))))))))

    (freeze html)))

# ── API: Plugins ──────────────────────────────────────────────────

(defn generate-plugins-html []
  "Generate HTML from plugin README.md files."
  (let* [result (subprocess/system "find" ["plugins" "-maxdepth" "2" "-name" "README.md" "-type" "f"])
         files (filter (fn [f] (and (not (= f ""))
                                     (not (= f "plugins/README.md"))))
                  (string/split result:stdout "\n"))]
    (def @html @"")
    (push html "<p>Available plugins. Each provides primitives accessible after import.</p>\n")

    (each file in (sort-with (fn [a b] (if (< a b) -1 (if (> a b) 1 0))) files)
      (let* [source (slurp file)
             parsed (parse-markdown source)
             # Extract plugin dir: plugins/name/README.md → plugins/name
             plugin-dir (let* [parts (string/split file "/")
                                dir-name (get parts 1)]
                           (string "plugins/" dir-name))]
        (push html (string "<h2>" (html-escape parsed:title)
          " <a class=\"source-link\" href=\"" github-base "/" plugin-dir "\">source</a>"
          " <a class=\"source-link\" href=\"" github-base "/" file "\">README</a></h2>\n"))
        (push html parsed:body)
        (push html "<hr>\n")))

    (freeze html)))

# ── Main generator ─────────────────────────────────────────────────

# Create output directory
(when (not (path/dir? output-dir))
  (create-directory-all output-dir))

# Read site configuration
(println "Reading site configuration...")
(def site-config (json-parse (slurp (path/join docs-dir "site.json"))))
(def site-title (get site-config "title"))
(def site-desc (get site-config "description"))

# Write CSS
(println "Generating CSS...")
(spit (path/join output-dir "style.css") (generate-css))

# Build all pages and navigation
(def @all-pages @[])
(def @nav-items @[])
(def @slug-map @{})

# Home page from docs/README.md
(let* [home-name (get site-config "home")
       home-file (path/join docs-root (string home-name ".md"))
       parsed (parse-markdown (slurp home-file))]
  (push nav-items {:slug "index" :title "Home"})
  (push all-pages {:slug "index" :title parsed:title
                   :description parsed:description :body parsed:body
                   :source-dir ""})
  (put slug-map "README" "index"))

# Process each section
(each section in (get site-config "sections")
  (let* [name (get section "name")
         dir (get section "dir")
         api (get section "api")
         pages (get section "pages")]

    (push nav-items {:section name})

    (if api
      # API Reference section — auto-generated pages
      (each page-name in pages
        (let* [slug (string "api-" page-name)
               title (cond
                        (= page-name "primitives") "Primitives"
                        (= page-name "prelude") "Prelude Macros"
                        (= page-name "stdlib") "Standard Library"
                        (= page-name "libraries") "Libraries"
                        (= page-name "plugins") "Plugins"
                        true page-name)]
          (push nav-items {:slug slug :title title})
          (push all-pages {:slug slug :title title
                           :description (string title " API reference")
                           :api page-name :source-dir ""})))

      # Markdown section — read from docs/
      (each page-name in pages
        (let* [file-path (if dir
                            (path/join docs-root dir (string page-name ".md"))
                            (path/join docs-root (string page-name ".md")))
               slug (if dir
                       (if (= page-name "index") dir (string dir "-" page-name))
                       page-name)
               source-dir (or dir "")
               read-result (protect (slurp file-path))]
          (if (not (get read-result 0))
            (eprintln "  Warning: skipping missing file " file-path)
            (let [parsed (parse-markdown (get read-result 1))]
              (push nav-items {:slug slug :title parsed:title})
              (push all-pages {:slug slug :title parsed:title
                               :description parsed:description
                               :body parsed:body :source-dir source-dir})
              # Map relative paths to slugs for link rewriting
              (let [rel-path (if dir
                                (string dir "/" page-name)
                                page-name)]
                (put slug-map rel-path slug)
                (put slug-map (string rel-path ".md") slug)
                (when (= page-name "index")
                  (put slug-map dir slug))))))))))

(def frozen-nav (freeze nav-items))
(def frozen-slug-map (freeze slug-map))

# Generate all pages
(each page in all-pages
  (let* [slug (get page :slug)
         title (get page :title)
         desc (get page :description)
         source-dir (get page :source-dir)
         api-name (get page :api)]
    (def @body-html (if api-name
        # Auto-generated API content
        (cond
          (= api-name "primitives") (generate-primitives-html)
          (= api-name "prelude") (generate-prelude-html)
          (= api-name "stdlib") (generate-stdlib-html)
          (= api-name "libraries") (generate-libraries-html)
          (= api-name "plugins") (generate-plugins-html)
          true "")
        # Markdown content with link rewriting
        (rewrite-md-links (get page :body) source-dir frozen-slug-map)))

    (let [full-html (generate-page site-title title desc slug frozen-nav body-html)]
      (spit (path/join output-dir (string slug ".html")) full-html))

    (println "Generating: " slug ".html...")))

(println "Generated " (string (length all-pages)) " pages in " output-dir "/")
