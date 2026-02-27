;; Page template generation

;; Generate navigation HTML using fold
(var generate-nav
  (fn (nav-items current-slug)
    (fold
      (fn (acc item)
        (let ((slug (get item "slug"))
              (title (get item "title"))
              (active-class (if (string-contains? slug current-slug) " active" "")))
          (-> acc (append "<li><a href=\"") (append slug) (append ".html\" class=\"nav-link") (append active-class) (append "\">") (append title) (append "</a></li>"))))
      ""
      nav-items)))

;; Generate the full HTML page
(var generate-page
  (fn (site page nav css body)
    (let ((site-title (get site "title"))
          (page-title (get page "title"))
          (page-desc (get page "description"))
          (nav-items (get site "nav"))
          (current-slug (get page "slug")))
      (-> "<!DOCTYPE html>\n"
        (append "<html lang=\"en\">\n")
        (append "<head>\n")
        (append "  <meta charset=\"UTF-8\">\n")
        (append "  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n")
        (append "  <title>") (append page-title) (append " - ") (append site-title) (append "</title>\n")
        (append "  <meta name=\"description\" content=\"") (append page-desc) (append "\">\n")
        (append "  <style>\n")
        (append css)
        (append "  </style>\n")
        (append "</head>\n")
        (append "<body>\n")
        (append "  <nav class=\"sidebar\">\n")
        (append "    <div class=\"site-title\">") (append site-title) (append "</div>\n")
        (append "    <ul>\n")
        (append (generate-nav nav-items current-slug))
        (append "    </ul>\n")
        (append "  </nav>\n")
        (append "  <main class=\"content\">\n")
        (append "    <h1>") (append page-title) (append "</h1>\n")
        (append body)
        (append "  </main>\n")
        (append "</body>\n")
        (append "</html>\n")))))
