;; Page template generation

;; Generate navigation HTML
(define generate-nav
  (lambda (nav-items current-slug)
    (define render-nav-items
      (lambda (items result)
        (if (empty? items)
          result
          (let ((item (first items)))
            (let ((slug (get item "slug"))
                  (title (get item "title"))
                  (active-class (if (string-contains? slug current-slug) " active" "")))
              (render-nav-items (rest items)
                (string-append result 
                  "<li><a href=\"" slug ".html\" class=\"nav-link" active-class "\">" 
                  title "</a></li>")))))))
    (render-nav-items items "")))

;; Generate the full HTML page
(define generate-page
  (lambda (site page nav css body)
    (let ((site-title (get site "title"))
          (page-title (get page "title"))
          (page-desc (get page "description"))
          (nav-items (get site "nav"))
          (current-slug (get page "slug")))
      (string-append
        "<!DOCTYPE html>\n"
        "<html lang=\"en\">\n"
        "<head>\n"
        "  <meta charset=\"UTF-8\">\n"
        "  <meta name=\"viewport\" content=\"width=device-width, initial-scale=1.0\">\n"
        "  <title>" page-title " - " site-title "</title>\n"
        "  <meta name=\"description\" content=\"" page-desc "\">\n"
        "  <style>\n"
        css
        "  </style>\n"
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
