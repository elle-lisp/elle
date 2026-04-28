(elle/epoch 9)
## Markdown-to-HTML parser for the documentation generator
##
## Parses standard markdown: headings, code fences, tables, lists,
## blockquotes, paragraphs, horizontal rules, inline formatting.
## Returns {:title str :body html-str :description str}.

(fn []

  # ── HTML escaping ──────────────────────────────────────────────────

  (defn html-escape [str]
    (if (or (nil? str) (not (string? str)))
      (if (nil? str) "" (string str))
      (-> str
          (string-replace "&" "&amp;")
          (string-replace "<" "&lt;")
          (string-replace ">" "&gt;")
          (string-replace (string 34) "&quot;")
          (string-replace "'" "&#39;"))))

  # ── Inline formatting ──────────────────────────────────────────────

  (defn find-closing [text start delimiter]
    "Find delimiter in text starting from character position."
    (let* [dlen (length delimiter)
           tlen (length text)]
      (def @pos start)
      (def @result nil)
      (while (< pos tlen)
        (when (and (<= (+ pos dlen) tlen)
            (= (slice text pos (+ pos dlen)) delimiter))
          (assign result pos)
          (break))
        (assign pos (+ pos 1)))
      result))
  (defn format-links [text]
    "Convert [text](url) to HTML anchor tags."
    (def @result "")
    (def @src text)
    (while (not (= src ""))
      (let [bp (find-closing src 0 "[")]
        (if (nil? bp)
          (begin
            (assign result (append result src))
            (assign src ""))
          (let [cb (find-closing src (+ bp 1) "]")]
            (if (nil? cb)
              (begin
                (assign result (append result src))
                (assign src ""))
              (if (or (>= (+ cb 1) (length src))
                  (not (= (slice src (+ cb 1) (+ cb 2)) "(")))
                (begin
                  (assign result (append result (slice src 0 (+ cb 1))))
                  (assign src (slice src (+ cb 1) (length src))))
                (let [cp (find-closing src (+ cb 2) ")")]
                  (if (nil? cp)
                    (begin
                      (assign result (append result src))
                      (assign src ""))
                    (begin
                      (assign
                        result
                        (string result (slice src 0 bp) "<a href=\""
                          (slice src (+ cb 2) cp) "\">" (slice src (+ bp 1) cb)
                          "</a>"))
                      (assign src (slice src (+ cp 1) (length src))))))))))))
    result)
  (defn apply-formatting [parts tag]
    "Apply HTML tag to alternating parts (bold/italic/code)."
    (first (fold (fn [state part]
                   [(if (first (rest state))
                      (string (first state) "<" tag ">" part "</" tag ">")
                      (append (first state) part)) (not (first (rest state)))])
             ["" false] parts)))
  (defn format-inline [text]
    "Format inline markdown: **bold**, *italic*, `code`, [links](url)."
    (let* [escaped (html-escape text)
           bold (apply-formatting (string/split escaped "**") "strong")
           italic (apply-formatting (string/split bold "*") "em")
           code (apply-formatting (string/split italic "`") "code")]
      (format-links code)))

  # ── Block parsing helpers ──────────────────────────────────────────

  (defn heading-level [line]
    "Return heading level (1-6) or nil."
    (if (not (string/starts-with? line "#"))
      nil
      (let* [len (length line)]
        (def @level 0)
        (while (and (< level len) (= (slice line level (+ level 1)) "#"))
          (assign level (+ level 1)))
        (if (and (<= level 6) (< level len)
            (= (slice line level (+ level 1)) " "))
          level
          nil))))
  (defn is-separator-row? [line]
    "Check if a table line is a separator row (|---|---|)."
    (let [cleaned (-> line
                      (string-replace "|" "")
                      (string-replace "-" "")
                      (string-replace ":" "")
                      (string-replace " " ""))]
      (= cleaned "")))
  (defn parse-table-cells [line]
    "Split a markdown table row into trimmed cells."
    (let* [trimmed (string/trim line)
           inner (if (string/starts-with? trimmed "|")
                   (slice trimmed 1 (length trimmed))
                   trimmed)
           inner (if (string/ends-with? inner "|")
                   (slice inner 0 (- (length inner) 1))
                   inner)]
      (map string/trim (string/split inner "|"))))
  (defn is-list-item? [line]
    "Check if line starts an unordered list item."
    (or (string/starts-with? line "- ") (string/starts-with? line "* ")))
  (defn is-block-boundary? [line]
    "Check if line starts a new block (heading, fence, table, list, quote, rule)."
    (let [trimmed (string/trim line)]
      (or (= trimmed "") (not (nil? (heading-level line)))
        (string/starts-with? line "```") (string/starts-with? trimmed "|")
        (is-list-item? line) (string/starts-with? line "> ") (= trimmed "---")
        (= trimmed "***") (= trimmed "___"))))

  # ── Main parser ────────────────────────────────────────────────────

  (defn parse [text]
    "Parse markdown text. Returns {:title str :body str :description str}."
    (let [lines (string/split text "\n")]
      (def @n (length lines))
      (def @i 0)
      (def @title nil)
      (def @desc nil)
      (def @body (thaw ""))
      (while (< i n)
        (let [line (get lines i)]
          (cond  # ── Blank line ──
            (= (string/trim line) "") (assign i (+ i 1))

            # ── Code fence ──
            (string/starts-with? line "```")
              (let [lang (string/trim (slice line 3 (length line)))]
                (assign i (+ i 1))
                (def @code-lines @[])
                (while (and (< i n)
                    (not (string/starts-with? (get lines i) "```")))
                  (push code-lines (get lines i))
                  (assign i (+ i 1)))
                (when (< i n) (assign i (+ i 1)))
                (push body
                  (string "<pre><code class=\"language-"
                    (html-escape (if (= lang "") "text" lang)) "\">"
                    (html-escape (string/join (freeze code-lines) "\n"))
                    "</code></pre>\n")))

              # ── Heading ──
              (heading-level line)
              (let* [level (heading-level line)
                     htext (string/trim (slice line (+ level 1) (length line)))]
                (if (and (= level 1) (nil? title))  # First h1 becomes page title; following paragraph is description
                  (begin
                    (assign title htext)
                    (assign i (+ i 1))  # Capture description from first paragraph after title
                    (when (and (< i n) (nil? desc)
                        (not (= (string/trim (get lines i)) ""))
                        (nil? (heading-level (get lines i)))
                        (not (string/starts-with? (get lines i) "```")))
                      (def @desc-lines @[])
                      (while (and (< i n)
                          (not (= (string/trim (get lines i)) ""))
                          (not (is-block-boundary? (get lines i))))
                        (push desc-lines (get lines i))
                        (assign i (+ i 1)))
                      (assign desc (string/join (freeze desc-lines) " "))
                      (push body (string "<p>" (format-inline desc) "</p>\n"))))
                  (begin
                    (let [id (-> htext
                                 (string-replace " " "-")
                                 (string-replace "(" "")
                                 (string-replace ")" "")
                                 (string-replace "/" "-")
                                 (string-replace "'" "")
                                 (string-replace "," ""))]
                      (push body
                        (string "<h" (string level) " id=\"" (html-escape id)
                          "\">" (format-inline htext) "</h" (string level) ">\n")))
                    (assign i (+ i 1)))))

              # ── Table ──
              (string/starts-with? (string/trim line) "|")
              (begin
                (def @table-lines @[])
                (while (and (< i n)
                    (string/starts-with? (string/trim (get lines i)) "|"))
                  (push table-lines (get lines i))
                  (assign i (+ i 1)))
                (let [tlines (freeze table-lines)]
                  (when (>= (length tlines) 2)
                    (let* [headers (parse-table-cells (get tlines 0))
                           has-sep (and (>= (length tlines) 2)
                             (is-separator-row? (get tlines 1)))
                           data-start (if has-sep 2 1)
                           data-rows (slice tlines data-start (length tlines))]
                      (push body "<table><thead><tr>")
                      (each cell in headers
                        (push body (string "<th>" (format-inline cell) "</th>")))
                      (push body "</tr></thead><tbody>")
                      (each row-line in data-rows
                        (push body "<tr>")
                        (each cell in (parse-table-cells row-line)
                          (push body
                            (string "<td>" (format-inline cell) "</td>")))
                        (push body "</tr>"))
                      (push body "</tbody></table>\n")))))

              # ── Unordered list ──
              (is-list-item? line)
              (begin
                (def @items @[])
                (while (and (< i n) (is-list-item? (get lines i)))
                  (push items (slice (get lines i) 2 (length (get lines i))))
                  (assign i (+ i 1)))
                (push body "<ul>")
                (each item in items
                  (push body
                    (string "<li>" (format-inline (string/trim item)) "</li>")))
                (push body "</ul>\n"))

              # ── Blockquote ──
              (string/starts-with? line "> ")
              (begin
                (def @quote-lines @[])
                (while (and (< i n) (string/starts-with? (get lines i) "> "))
                  (push quote-lines
                    (slice (get lines i) 2 (length (get lines i))))
                  (assign i (+ i 1)))
                (push body
                  (string "<blockquote><p>"
                    (format-inline (string/join (freeze quote-lines) " "))
                    "</p></blockquote>\n")))

              # ── Horizontal rule ──
              (or (= (string/trim line) "---") (= (string/trim line) "***")
              (= (string/trim line) "___"))
              (begin
                (push body "<hr>\n")
                (assign i (+ i 1)))

              # ── Paragraph ──
              true
              (begin
                (def @para-lines @[])
                (while (and (< i n) (not (is-block-boundary? (get lines i))))
                  (push para-lines (get lines i))
                  (assign i (+ i 1)))
                (when (not (empty? para-lines))
                  (push body
                    (string "<p>"
                      (format-inline (string/join (freeze para-lines) " "))
                      "</p>\n")))))))
      {:title (or title "Untitled")
       :body (freeze body)
       :description (or desc "")}))

  # ── Export ──────────────────────────────────────────────────────────

  {:parse parse
   :format-inline format-inline
   :html-escape html-escape
   :find-closing find-closing})
