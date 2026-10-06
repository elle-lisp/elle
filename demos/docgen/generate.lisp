(elle/epoch 14)
## audited: 2026-10-06
## The documentation site: the markdown documents as HTML pages, and an API reference with each function's signals.
## demos/docgen/README.md

# ── Configuration ──────────────────────────────────────────────────

(def @docs-root "docs")
(def @output-dir "site")
(def @docs-dir "demos/docgen/docs")
# The Elle sources whose comments and defns become the API reference.
# Every path here is resolved against the repository root, which is where
# the generator is invoked from.
(def @src-dir "src")
(def @prelude-path (path/join src-dir "prelude.lisp"))
(def @stdlib-path (path/join src-dir "stdlib.lisp"))
# Every stdlib entry carries a source link built on this. Cargo.toml's
# `repository` is the canonical spelling of the URL; tests/integration/paths.rs
# pins the two together.
(def @github-base "https://github.com/elle-lisp/elle/blob/main")

# ── Imports ────────────────────────────────────────────────────────

(def md ((import "./lib/markdown")))
(def css-mod ((import "./lib/css")))

(def @html-escape md:html-escape)
(def @format-inline md:format-inline)
(def @parse-markdown md:parse)
(def @generate-css css-mod:generate-css)
(def api
  ((import "./lib/api") {:html-escape html-escape
                         :format-inline format-inline
                         :parse-markdown parse-markdown
                         :github-base github-base
                         :prelude-path prelude-path
                         :stdlib-path stdlib-path}))

# ── Navigation ─────────────────────────────────────────────────────

(defn render-nav [nav-items current-slug]
  "Generate sidebar HTML from nav items."
  (def @html @"")
  (each item in nav-items
    (if (get item :section)
      (push html
            (string "      <li class=\"nav-section\">"
                    (html-escape (get item :section)) "</li>\n"))
      (let* [slug (get item :slug)
             title (get item :title)
             active (if (= slug current-slug) " active" "")]
        (push html
              (string "      <li><a href=\"" slug ".html\" class=\"nav-link"
                      active "\">" (html-escape title) "</a></li>\n")))))
  (freeze html))

# ── Page template ──────────────────────────────────────────────────

(defn
  generate-page
  [site-title page-title page-desc current-slug nav-items body]
  "Generate a complete HTML page."
  (string "<!DOCTYPE html>\n<html lang=\"en\">\n<head>\n"
          "  <meta charset=\"UTF-8\">\n"
          "  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n"
          "  <title>" (html-escape page-title) " - " (html-escape site-title)
          "</title>\n" "  <meta name=\"description\" content=\""
          (html-escape page-desc) "\">\n"
          "  <link rel=\"stylesheet\" href=\"style.css\">\n" "</head>\n<body>\n"
          "  <nav class=\"sidebar\">\n" "    <div class=\"site-title\">"
          (html-escape site-title) "</div>\n" "    <ul>\n"
          (render-nav nav-items current-slug) "    </ul>\n" "  </nav>\n"
          "  <main class=\"content\">\n" "    <h1>" (html-escape page-title)
          "</h1>\n" body "  </main>\n" "</body>\n</html>\n"))

# ── Link rewriting ─────────────────────────────────────────────────

(defn rewrite-md-links [html source-dir slug-map]
  "Rewrite .md href links to .html using the slug map."
  (def @result @"")
  (def @pos 0)
  (def @len (length html))
  (while (< pos len)
    (let [found (string/find html "href=\"" pos)]
      (if (nil? found)
        (begin
          (push result (slice html pos len))
          (assign pos len))
        (let* [href-start (+ found 6)
               href-end (string/find html "\"" href-start)]
          (if (nil? href-end)
            (begin
              (push result (slice html pos len))
              (assign pos len))
            (let [url (slice html href-start href-end)]
              (if (not (string/ends-with? url ".md"))  # Not a .md link: keep as-is
                (begin
                  (push result (slice html pos (+ href-end 1)))
                  (assign pos (+ href-end 1)))  # Rewrite .md link
                (let* [md-path (slice url 0 (- (length url) 3))
                       resolved (if (= source-dir "")
                                  md-path
                                  (cond
                                    (string/starts-with? md-path "../") (slice md-path
                                    3 (length md-path))
                                    (string-contains? md-path "/") md-path
                                    true (string source-dir "/" md-path)))
                       slug (or (get slug-map resolved) resolved)]
                  (push result (slice html pos href-start))
                  (push result slug)
                  (push result ".html")
                  (assign pos href-end)))))))))
  (freeze result))

# ── Main generator ─────────────────────────────────────────────────

# Create output directory
(when (not (path/dir? output-dir)) (create-directory-all output-dir))

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
  (push all-pages
        {:slug "index"
         :title parsed:title
         :description parsed:description
         :body parsed:body
         :source-dir ""})
  (put slug-map "README" "index"))

# Process each section
(each section in (get site-config "sections")
  (let* [name (get section "name")
         dir (get section "dir")
         api (get section "api")
         pages (get section "pages")]
    (push nav-items {:section name})

    (if api  # API Reference section — auto-generated pages
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
          (push all-pages
                {:slug slug
                 :title title
                 :description (string title " API reference")
                 :api page-name
                 :source-dir ""})))

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
              (push all-pages
                    {:slug slug
                     :title parsed:title
                     :description parsed:description
                     :body parsed:body
                     :source-dir source-dir})  # Map relative paths to slugs for link rewriting
              (let [rel-path (if dir (string dir "/" page-name) page-name)]
                (put slug-map rel-path slug)
                (put slug-map (string rel-path ".md") slug)
                (when (= page-name "index") (put slug-map dir slug))))))))))

(def frozen-nav (freeze nav-items))
(def frozen-slug-map (freeze slug-map))

# Generate all pages
(each page in all-pages
  (let* [slug (get page :slug)
         title (get page :title)
         desc (get page :description)
         source-dir (get page :source-dir)
         api-name (get page :api)]
    (def @body-html
      (if api-name  # Auto-generated API content
        (cond
          (= api-name "primitives") (api:primitives)
          (= api-name "prelude") (api:prelude)
          (= api-name "stdlib") (api:stdlib)
          (= api-name "libraries") (api:libraries)
          (= api-name "plugins") (api:plugins)
          true "")  # Markdown content with link rewriting
        (rewrite-md-links (get page :body) source-dir frozen-slug-map)))

    (let [full-html (generate-page site-title title desc slug frozen-nav
                                   body-html)]
      (spit (path/join output-dir (string slug ".html")) full-html))

    (println "Generating: " slug ".html...")))

(println "Generated " (string (length all-pages)) " pages in " output-dir "/")
