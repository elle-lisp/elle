(elle/epoch 15)
## audited: 2026-10-21
## The API pages that show each function's signals: the stdlib and the libraries.
## demos/docgen/README.md
##
## The module takes the site's helpers and paths, and answers one page renderer
## for each page. Both pages analyze Elle source with compile/analyze.

(fn [site]
  (def html-escape site:html-escape)
  (def format-inline site:format-inline)
  (def github-base site:github-base)
  (def stdlib-path site:stdlib-path)

  # ── Signal display ─────────────────────────────────────────────────

  (defn signal-class [bit]
    "CSS class for a signal keyword."
    (cond
      (= bit :error) "signal-error"
      (= bit :io) "signal-io"
      (= bit :yield) "signal-yield"
      (= bit :exec) "signal-exec"
      (= bit :ffi) "signal-ffi"
      (= bit :fuel) "signal-fuel"
      (= bit :wait) "signal-wait"
      true "signal-badge"))

  (defn render-signal-badges [sig]
    "Render signal profile as HTML badges."
    (if (get sig :silent)
      "<span class=\"signal-badge signal-silent\">silent</span>"
      (let [bits (get sig :bits)]
        (if (empty? bits)
          ""
          (let [result @""]
            (each bit in (set->array bits)
              (push result
                    (string "<span class=\"signal-badge " (signal-class bit)
                            "\">" (string bit) "</span>")))
            (freeze result))))))

  # ── API: Stdlib functions with signal profiles ─────────────────────

  (defn extract-section-name [line]
    "Extract section name from ## ── Name ── line."
    (when (string/starts-with? line "## ")
      (let [text (slice line 3 (length line))]
        (when (string-contains? text "──")
          (let* [parts (string/split text "──")
                 name (string/trim (get parts 1))]
            (when (not (= name "")) name))))))

  (defn generate-stdlib-html []
    "Generate HTML for stdlib functions with signal profiles."
    (let* [source (slurp stdlib-path)
           analysis (compile/analyze source {:file stdlib-path})
           lines (string/split source "\n")]
      (def @html @"")
      (push html
            "<p>Runtime functions loaded at startup after primitives.</p>\n")

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
            (string/starts-with? line "(def ") (assign comment-lines @[])
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
              (let* [sig-result (protect (compile/signal analysis
                     (keyword fn-name)))
                     sig (when (get sig-result 0) (get sig-result 1))
                     desc (or (get fn-comments fn-name) "")
                     line-num (get fn-lines fn-name)
                     source-link (when line-num
                                   (string github-base "/" stdlib-path "#L"
                                   (string line-num)))]
                (push html "<div class=\"api-entry\">")
                (push html
                      (string "<code class=\"api-signature\">"
                              (html-escape fn-name) "</code>"))
                (when source-link
                  (push html
                        (string " <a class=\"source-link\" href=\"" source-link
                                "\">src</a>")))
                (when sig
                  (push html
                        (string "\n<div class=\"api-signals\">"
                                (render-signal-badges sig) "</div>")))
                (when (not (= desc ""))
                  (push html
                        (string "<p class=\"api-desc\">" (format-inline desc)
                                "</p>")))
                (push html "</div>\n"))))))

      (freeze html)))

  # ── API: Libraries ────────────────────────────────────────────────

  (defn generate-libraries-html []
    "Generate HTML for lib/*.lisp modules."
    (let* [result (subprocess/system "find"
                                     ["lib" "-maxdepth" "1" "-name" "*.lisp"
                                      "-type" "f"])
           files (filter (fn [f] (not (= f "")))
                         (string/split result:stdout "\n"))]
      (def @html @"")
      (push html
            "<p>Reusable libraries in <code>lib/</code>. Each exports a struct of functions.</p>\n")

      (each file in (sort-with (fn [a b] (if (< a b) -1 (if (> a b) 1 0))) files)
        (let* [source (slurp file)
               lines (string/split source "\n")  # Extract first ## comment as description
               desc (let [@first-comment nil]
                      (each line in lines
                        (when (and (nil? first-comment)
                                   (string/starts-with? line "## "))
                          (assign first-comment (slice line 3 (length line)))))
                      first-comment)  # Extract module name from filename
               basename (let* [parts (string/split file "/")
                               fname (get parts (- (length parts) 1))]
                          (slice fname 0 (- (length fname) 5)))  # Try to analyze
               analysis-result (protect (compile/analyze source {:file file}))]
          (push html
                (string "<h2>" (html-escape basename)
                        " <a class=\"source-link\" href=\"" github-base "/" file
                        "\">source</a></h2>\n"))
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
                                         (= (get s :kind) :function))) syms)]
                  (when (and (array? fn-syms) (not (empty? fn-syms)))
                    (push html
                          "<table><thead><tr><th>Export</th><th>Signals</th></tr></thead><tbody>\n")
                    (each sym in fn-syms
                      (let* [name (get sym :name)
                             line-num (get sym :line)
                             sig-result (protect (compile/signal analysis
                             (keyword name)))
                             badges (if (get sig-result 0)
                                      (render-signal-badges (get sig-result 1))
                                      "")]
                        (push html
                              (string "<tr><td><code>" (html-escape name)
                                      "</code>"
                                      (if line-num
                                        (string " <a class=\"source-link\" href=\""
                                        github-base "/" file "#L"
                                        (string line-num) "\">src</a>")
                                        "") "</td><td>" badges "</td></tr>\n"))))
                    (push html "</tbody></table>\n"))))))))

      (freeze html)))

  {:stdlib generate-stdlib-html :libraries generate-libraries-html})
