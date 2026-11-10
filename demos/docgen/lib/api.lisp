(elle/epoch 15)
## audited: 2026-10-21
## The API reference pages: primitives, prelude macros, the stdlib, libraries and plugins.
## demos/docgen/README.md
##
## The module takes the site's helpers and paths, and answers one page renderer
## for each page. The stdlib and library pages come from
## demos/docgen/lib/profiles.lisp.

(fn [site]
  (def html-escape site:html-escape)
  (def format-inline site:format-inline)
  (def parse-markdown site:parse-markdown)
  (def github-base site:github-base)
  (def prelude-path site:prelude-path)
  (def profiles ((import "./profiles") site))

  # ── API: Primitives ───────────────────────────────────────────────

  (defn category-display-name [cat]
    (cond
      (= cat "") "Core"
      (= cat "math") "Math"
      (= cat "string") "String Operations"
      (= cat "file") "File I/O"
      (= cat "ffi") "FFI (Foreign Function Interface)"
      (= cat "fiber") "Fibers"
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

  (def @category-order
    ["" "math" "string" "array" "struct" "json" "file" "fn" "fiber" "clock"
     "time" "meta" "debug" "bit" "os" "pkg" "module" "ffi"])

  (defn generate-primitives-html []
    "Generate HTML for the primitives API page."
    (let* [names (vm/list-primitives)
           groups (@struct)]
      (each name in names
        (let* [meta (vm/primitive-meta name)
               canonical (get meta :name)]
          (when (= (string name) canonical)
            (let* [cat (get meta :category)
                   existing (get groups cat)
                   items (if (nil? existing) @[] existing)]
              (push items meta)
              (put groups cat items)))))

      (def @html @"")  # Render in preferred order
      (each cat in category-order
        (let [metas (get groups cat)]
          (when (and (not (nil? metas)) (not (empty? metas)))
            (push html (string "<h2>" (category-display-name cat) "</h2>\n"))
            (push html
                  "<table><thead><tr><th>Function</th><th>Description</th><th>Example</th></tr></thead><tbody>\n")
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
                            (string doc " (alias: " (string/join aliases ", ")
                                    ")"))
                     example (get meta :example)]
                (push html
                      (string "<tr><td><code>" (html-escape sig-str)
                              "</code></td>" "<td>" (html-escape desc) "</td>"
                              "<td><code>" (html-escape (or example ""))
                              "</code></td></tr>\n"))))
            (push html "</tbody></table>\n"))))

      # Handle any categories not in the preferred order
      (each [cat metas] in (pairs groups)
        (when (not (any? (fn [c] (= c cat)) category-order))
          (push html (string "<h2>" (category-display-name cat) "</h2>\n"))
          (push html
                "<table><thead><tr><th>Function</th><th>Description</th><th>Example</th></tr></thead><tbody>\n")
          (each meta in metas
            (let* [name (get meta :name)
                   params (get meta :params)
                   sig-str (if (empty? params)
                             (string "(" name ")")
                             (string "(" name " " (string/join params " ") ")"))
                   doc (get meta :doc)
                   example (get meta :example)]
              (push html
                    (string "<tr><td><code>" (html-escape sig-str)
                            "</code></td>" "<td>" (html-escape doc) "</td>"
                            "<td><code>" (html-escape (or example ""))
                            "</code></td></tr>\n"))))
          (push html "</tbody></table>\n")))

      (freeze html)))

  # ── API: Prelude macros ───────────────────────────────────────────

  (defn generate-prelude-html []
    "Generate HTML for prelude macros."
    (let [source (slurp prelude-path)]
      (def @html @"")
      (push html
            "<p>Macros loaded automatically before user code. These expand at compile time.</p>\n")
      (push html
            "<table><thead><tr><th>Macro</th><th>Description</th></tr></thead><tbody>\n")

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
                       source-link (string github-base "/" prelude-path "#L"
                       (string line-num))]
                  (push html
                        (string "<tr><td><code>" (html-escape name) "</code>"
                                " <a class=\"source-link\" href=\"" source-link
                                "\">src</a></td>" "<td>" (format-inline desc)
                                "</td></tr>\n"))
                  (assign comment-lines @[]))
              true
                (when (not (= (string/trim line) "")) (assign comment-lines @[]))))
          (assign i (+ i 1))))

      (push html "</tbody></table>\n")
      (freeze html)))

  # ── API: Plugins ──────────────────────────────────────────────────

  (defn generate-plugins-html []
    "Generate HTML from plugin README.md files."
    (let* [result (subprocess/system "find"
                                     ["plugins" "-maxdepth" "2" "-name"
                                      "README.md" "-type" "f"])
           files (filter (fn [f]
                           (and (not (= f "")) (not (= f "plugins/README.md"))))
                         (string/split result:stdout "\n"))]
      (def @html @"")
      (push html
            "<p>Available plugins. Each provides primitives accessible after import.</p>\n")

      (each file in (sort-with (fn [a b] (if (< a b) -1 (if (> a b) 1 0))) files)
        (let* [source (slurp file)
               parsed (parse-markdown source)  # Extract plugin dir: plugins/name/README.md → plugins/name
               plugin-dir (let* [parts (string/split file "/")
                                 dir-name (get parts 1)]
                            (string "plugins/" dir-name))]
          (push html
                (string "<h2>" (html-escape parsed:title)
                        " <a class=\"source-link\" href=\"" github-base "/"
                        plugin-dir "\">source</a>"
                        " <a class=\"source-link\" href=\"" github-base "/" file
                        "\">README</a></h2>\n"))
          (push html parsed:body)
          (push html "<hr>\n")))

      (freeze html)))

  {:primitives generate-primitives-html
   :prelude generate-prelude-html
   :stdlib profiles:stdlib
   :libraries profiles:libraries
   :plugins generate-plugins-html})
